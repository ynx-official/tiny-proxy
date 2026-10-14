use crate::{
    auth::{
        codex::{Model, Service, VERIFICATION_URL, now},
        probe::ProbeResult,
    },
    gateway::{Gateway, Record, validate_bind},
    storage::{AccountSummary, CreatedKey, FailureRecord, KeySummary, ModelTest, Store},
};
use std::collections::VecDeque;
use std::{path::PathBuf, thread::JoinHandle, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub mod appearance;
use appearance::ThemePreference;

pub enum Command {
    CheckUpdate,
    DownloadUpdate,
    CancelUpdateDownload,
    InstallUpdate,
    ConfigureUpdates(crate::updater::Preferences),
    SignIn,
    Cancel,
    RefreshModels,
    TestModel(String),
    CreateKey(String),
    RevokeKey(String),
    RevealKey { id: String, copy: bool },
    SelectAccount(String),
    RefreshUsage(String),
    StartGateway(String),
    StopGateway,
    HistoryPage { requests: i64, failures: i64 },
    RequestDetail(String),
    SetThemePreference(ThemePreference),
}
pub struct HistorySnapshot {
    pub requests: Vec<Record>,
    pub failures: Vec<FailureRecord>,
    pub request_count: i64,
    pub failure_count: i64,
    pub request_page: i64,
    pub failure_page: i64,
    pub write_failures: u64,
}
pub enum Event {
    Update(Box<crate::updater::Snapshot>),
    UpdateProgress(u64, u64),
    UpdateNotice(String),
    QuitForUpdate,
    Snapshot {
        keys: Vec<KeySummary>,
        codex_connected: bool,
        accounts: Vec<AccountSummary>,
        models: Vec<Model>,
        models_checked_at: Option<i64>,
        tests: Vec<ModelTest>,
        theme_preference: ThemePreference,
    },
    Status {
        message: String,
        busy: bool,
    },
    DeviceCode {
        code: Zeroizing<String>,
        expires_in: Duration,
    },
    Gateway {
        address: Option<String>,
    },
    History(Box<HistorySnapshot>),
    RequestDetail {
        record: Box<Record>,
        failure: Option<FailureRecord>,
    },
    GatewayBind(String),
    ThemePreferenceSaved(Result<ThemePreference, String>),
    OpenBrowser(Zeroizing<String>),
    KeyCreated(CreatedKey),
    KeyRevealed {
        key: CreatedKey,
        copy: bool,
    },
    UsageBusy(Option<String>),
    Models(Vec<Model>),
    TestResult(ProbeResult),
    Error(String),
}
pub struct Backend {
    pub commands: mpsc::Sender<Command>,
    pub events: Option<mpsc::Receiver<Event>>,
    shutdown: CancellationToken,
    thread: Option<JoinHandle<()>>,
}
impl Backend {
    pub fn start(database: PathBuf) -> std::io::Result<Self> {
        Self::start_with_legacy(database, vec![])
    }
    pub fn start_with_legacy(
        database: PathBuf,
        legacy_directories: Vec<PathBuf>,
    ) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel(16);
        let (sender, events) = mpsc::channel(32);
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone();
        let thread=std::thread::Builder::new().name("tiny-proxy-runtime".into()).spawn(move || {
            let runtime=match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(runtime)=>runtime,Err(_)=>{let _=sender.blocking_send(Event::Error("后台运行时初始化失败".into()));return;}
            };
            runtime.block_on(async {
                let mut cleanup_store = None;
                // A full UI queue cannot prevent closing the application.
                tokio::select! {biased;_=stop.cancelled()=>{},_=run(database,legacy_directories,receiver,sender,stop.clone(), &mut cleanup_store)=>{}}
                // The run future may be dropped while a full UI queue blocks it.
                // Close persisted in-flight rows even on that shutdown path.
                if let Some(store) = cleanup_store {
                    if store.interrupt_requests(chrono::Utc::now().timestamp_millis(), "application_shutdown").await.is_err() { eprintln!("request_shutdown_record_failed"); }
                    store.close().await;
                }
            });
            runtime.shutdown_timeout(Duration::from_secs(2));
        })?;
        Ok(Self {
            commands,
            events: Some(events),
            shutdown,
            thread: Some(thread),
        })
    }
    pub fn stop(&mut self) {
        self.shutdown.cancel();
        self.events.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.stop();
    }
}
enum Outcome {
    SignedIn,
    Models(Vec<Model>),
    Tested(ProbeResult),
}

/// Pin the account before writing history so a later selection cannot misattribute a probe.
pub async fn test_and_save(
    service: &std::sync::Arc<Service>,
    store: &Store,
    model: &str,
    cancel: CancellationToken,
) -> Result<ProbeResult, String> {
    let account = service.active_id().await.map_err(|e| e.to_string())?;
    let id = store
        .start_model_test(&account, model, now())
        .await
        .map_err(|_| "无法保存测试开始状态".to_owned())?;
    let result = tokio::select! {biased;
        _=cancel.cancelled()=>Err("测试已取消".to_owned()),
        r=crate::gateway::test_text_for(service,&account,model)=>r
    };
    let (text, error, input, output) = match &result {
        Ok(r) => (Some(r.text.as_str()), None, r.input_tokens, r.output_tokens),
        Err(e) => (None, Some(e.as_str()), None, None),
    };
    store
        .finish_model_test(&id, text, error, input, output, now())
        .await
        .map_err(|_| "测试已结束，但保存结果失败".to_owned())?;
    result
}
async fn snapshot(store: &Store, service: &Service, sender: &mpsc::Sender<Event>) -> bool {
    let state = async {
        let keys = store.list_keys().await.map_err(|_| ())?;
        let codex_connected = service.connected().await.map_err(|_| ())?;
        let accounts = store.accounts().await.map_err(|_| ())?;
        let active = accounts.iter().find(|a| a.active);
        let models = active
            .and_then(|a| a.models_json.as_deref())
            .map(serde_json::from_str::<Vec<Model>>)
            .transpose()
            .map_err(|_| ())?
            .unwrap_or_default();
        let models_checked_at = active.and_then(|a| a.models_checked_at);
        let tests = if let Some(account) = active {
            store.model_tests(&account.id).await.map_err(|_| ())?
        } else {
            vec![]
        };
        let theme_preference = store.theme_preference().await.map_err(|_| ())?;
        Ok::<_, ()>(Event::Snapshot {
            keys,
            codex_connected,
            accounts,
            models,
            models_checked_at,
            tests,
            theme_preference,
        })
    }
    .await;
    match state {
        Ok(event) => {
            if let Event::Snapshot {
                keys,
                accounts,
                models,
                tests,
                codex_connected,
                ..
            } = &event
            {
                eprintln!(
                    "local_state keys={} accounts={} models={} tests={} connected={}",
                    keys.len(),
                    accounts.len(),
                    models.len(),
                    tests.len(),
                    codex_connected
                );
            }
            sender.send(event).await.is_ok()
        }
        _ => {
            eprintln!("snapshot_load_failed");
            let _ = sender.send(Event::Error("无法读取本地数据".into())).await;
            false
        }
    }
}

async fn gateway_blocker(store: &Store, service: &Service) -> Result<Option<&'static str>, String> {
    if !service.connected().await.map_err(|e| e.to_string())? {
        return Ok(Some("请先完成 Codex 设备码登录"));
    }
    if store
        .list_keys()
        .await
        .map_err(|_| "无法读取密钥".to_owned())?
        .is_empty()
    {
        return Ok(Some("请先创建至少一个客户端密钥"));
    }
    Ok(None)
}

async fn start_gateway(
    value: &str,
    store: &Store,
    service: &std::sync::Arc<Service>,
) -> Result<Gateway, String> {
    if let Some(reason) = gateway_blocker(store, service).await? {
        return Err(reason.into());
    }
    let address = validate_bind(value)?;
    let mut gateway = Gateway::start(address, store.clone(), service.clone())
        .await
        .map_err(|e| format!("无法监听 {address}：{e}"))?;
    // Only a successful bind replaces the address used on the next launch.
    if store.save_gateway_bind(address).await.is_err() {
        gateway.stop().await;
        return Err("无法保存监听地址，网关已停止".into());
    }
    Ok(gateway)
}

async fn announce_gateway(gateway: &Gateway, sender: &mpsc::Sender<Event>, busy: bool) {
    let _ = sender
        .send(Event::Gateway {
            address: Some(gateway.address.to_string()),
        })
        .await;
    let _ = sender
        .send(Event::Status {
            message: "网关已启动，客户端可使用 Base URL 和独立密钥连接".into(),
            busy,
        })
        .await;
}
async fn history(
    store: &Store,
    gateway: Option<&Gateway>,
    sender: &mpsc::Sender<Event>,
    request_page: i64,
    failure_page: i64,
) {
    let state = async {
        let (request_count, failure_count) = store.record_counts().await?;
        let request_page = request_page.clamp(0, (request_count - 1).max(0) / 100);
        let failure_page = failure_page.clamp(0, (failure_count - 1).max(0) / 100);
        let mut requests = store.request_records(request_page * 100, 100).await?;
        if let Some(gateway) = gateway {
            for live in gateway.live_records() {
                if let Some(record) = requests.iter_mut().find(|r| r.id == live.id) {
                    *record = live;
                }
            }
        }
        Ok::<_, crate::storage::Error>(HistorySnapshot {
            requests,
            failures: store.failure_records(failure_page * 100, 100).await?,
            request_count,
            failure_count,
            request_page,
            failure_page,
            write_failures: gateway.map_or(0, Gateway::log_write_failures),
        })
    }
    .await;
    match state {
        Ok(state) => {
            let _ = sender.try_send(Event::History(Box::new(state)));
        }
        Err(_) => {
            let _ = sender.try_send(Event::Error(
                "请求记录读取失败，已有记录保留在本地数据库".into(),
            ));
        }
    }
}

async fn restore_gateway(
    store: &Store,
    service: &std::sync::Arc<Service>,
    sender: &mpsc::Sender<Event>,
) -> Result<Option<Gateway>, String> {
    let address = store
        .gateway_bind()
        .await
        .map_err(|_| "无法读取监听地址".to_owned())?;
    let _ = sender.send(Event::GatewayBind(address.clone())).await;
    if let Some(reason) = gateway_blocker(store, service).await? {
        let _ = sender
            .send(Event::Status {
                message: format!("本地数据已加载 · 网关未启动：{reason}"),
                busy: false,
            })
            .await;
        return Ok(None);
    }
    let gateway = start_gateway(&address, store, service).await?;
    announce_gateway(&gateway, sender, false).await;
    Ok(Some(gateway))
}

async fn run(
    database: PathBuf,
    legacy_directories: Vec<PathBuf>,
    mut commands: mpsc::Receiver<Command>,
    sender: mpsc::Sender<Event>,
    shutdown: CancellationToken,
    cleanup_store: &mut Option<Store>,
) {
    match crate::storage::recovery::recover_legacy(&database, &legacy_directories).await {
        Ok(Some(source)) => eprintln!("storage_recovered source={}", source.display()),
        Ok(None) => {}
        Err(error) => {
            let _ = sender.send(Event::Error(error.to_string())).await;
            return;
        }
    }
    let store = match Store::open(&database).await {
        Ok(s) => s,
        Err(error) => {
            let _ = sender
                .send(Event::Error(error.startup_message(&database)))
                .await;
            return;
        }
    };
    *cleanup_store = Some(store.clone());
    let service = match Service::new(store.clone()) {
        Ok(s) => s,
        Err(e) => {
            let _ = sender.send(Event::Error(e.to_string())).await;
            return;
        }
    };
    if service.initialize().await.is_err()
        || store.interrupt_model_tests().await.is_err()
        || store
            .interrupt_requests(
                chrono::Utc::now().timestamp_millis(),
                "application_interrupted",
            )
            .await
            .is_err()
    {
        let _ = sender
            .send(Event::Error("本地账号恢复失败，原有数据已保留".into()))
            .await;
        return;
    }
    if !snapshot(&store, &service, &sender).await {
        return;
    }
    let mut gateway = match restore_gateway(&store, &service, &sender).await {
        Ok(gateway) => gateway,
        Err(error) => {
            let _ = sender
                .send(Event::Error(format!("网关自动启动失败：{error}")))
                .await;
            None
        }
    };
    let mut jobs = JoinSet::<Result<Outcome, String>>::new();
    let mut updates =
        match crate::updater::runtime::Updates::new(&store, sender.clone(), shutdown.clone()).await
        {
            Ok(updates) => updates,
            Err(error) => {
                let _ = sender.send(Event::Error(error)).await;
                return;
            }
        };
    let mut operation = CancellationToken::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut usage_tick = tokio::time::interval(Duration::from_secs(60));
    usage_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut usage_queue = VecDeque::<String>::new();
    let mut usage_current: Option<String> = None;
    let mut usage_jobs = JoinSet::new();
    let mut request_page = 0;
    let mut failure_page = 0;
    history(
        &store,
        gateway.as_ref(),
        &sender,
        request_page,
        failure_page,
    )
    .await;
    loop {
        if !updates.installing()
            && usage_jobs.is_empty()
            && let Some(id) = usage_queue.pop_front()
        {
            usage_current = Some(id.clone());
            let _ = sender.send(Event::UsageBusy(Some(id.clone()))).await;
            let upstream = service.clone();
            usage_jobs.spawn(async move { upstream.refresh_usage(&id).await });
        }
        tokio::select! {
            biased;
            _=shutdown.cancelled()=>break,
            result=updates.jobs.join_next(),if !updates.jobs.is_empty()=>{
                if updates.complete(&store,result).await { break; }
            },
            result=jobs.join_next(),if !jobs.is_empty()=>{
                match result {
                    Some(Ok(Ok(Outcome::SignedIn)))=>{
                        if let Ok(id)=service.active_id().await && usage_current.as_ref()!=Some(&id) && !usage_queue.contains(&id){usage_queue.push_back(id);}
                        let _=sender.send(Event::Models(vec![])).await;
                        let _=sender.send(Event::Status{message:"Codex 授权凭据已安全保存。请读取模型目录并执行文本测试。".into(),busy:false}).await;
                    },
                    Some(Ok(Ok(Outcome::Models(models))))=>{
                        let _=sender.send(Event::Models(models)).await;
                        let _=sender.send(Event::Status{message:"Codex 模型目录已更新".into(),busy:false}).await;
                    },
                    Some(Ok(Ok(Outcome::Tested(result))))=>{
                        let _=sender.send(Event::TestResult(result)).await;
                        let _=sender.send(Event::Status{message:"Codex 文本测试完成，已收到 response.completed".into(),busy:false}).await;
                    },
                    Some(Ok(Err(e)))=>{let _=sender.send(Event::Error(e)).await;},
                    _=>{let _=sender.send(Event::Error("后台任务意外结束".into())).await;},
                }
                snapshot(&store,&service,&sender).await;
            },
            _=usage_jobs.join_next(),if !usage_jobs.is_empty()=>{
                usage_current=None;
                let _=sender.send(Event::UsageBusy(None)).await;
                snapshot(&store,&service,&sender).await;
            },
            _=usage_tick.tick(),if !updates.installing()=>{
                if store.prune_request_records(chrono::Utc::now().timestamp_millis() - 7 * 24 * 3600 * 1000, 10000).await.is_err() { let _=sender.try_send(Event::Error("过期请求记录清理失败".into())); }
                if let Ok(ids)=store.due_accounts(now()).await {
                    for id in ids {if usage_current.as_ref()!=Some(&id) && !usage_queue.contains(&id){usage_queue.push_back(id);}}
                }
            },
            command=commands.recv()=>{
                let Some(command)=command else {break};
                if updates.installing() { continue; }
                match command {
                    Command::CheckUpdate=>updates.check(),
                    Command::DownloadUpdate=>updates.download(),
                    Command::CancelUpdateDownload=>updates.cancel_download(),
                    Command::ConfigureUpdates(preferences)=>updates.configure(&store,preferences).await,
                    Command::InstallUpdate=>{
                        if let Some((info,path))=updates.ready() {
                            let _=sender.send(Event::Status{message:"已确认安装，正在停止网关并收尾记录…".into(),busy:true}).await;
                            operation.cancel();jobs.abort_all();usage_jobs.abort_all();
                            while jobs.join_next().await.is_some() {}
                            while usage_jobs.join_next().await.is_some() {}
                            usage_current=None;usage_queue.clear();
                            let _=sender.send(Event::UsageBusy(None)).await;
                            if let Some(mut g)=gateway.take(){g.stop().await;let _=sender.send(Event::Gateway{address:None}).await;}
                            if store.interrupt_requests(chrono::Utc::now().timestamp_millis(),"application_update").await.is_err() || store.interrupt_model_tests().await.is_err() {
                                let _=sender.send(Event::Error("更新前无法收尾记录；请重试".into())).await;
                            } else { updates.install(info,path); }
                        }
                    },
                    Command::SetThemePreference(preference)=>{
                        let result=store.save_theme_preference(preference).await
                            .map(|()|preference)
                            .map_err(|_|"主题设置保存失败，已保留原主题，请重试".to_owned());
                        let _=sender.send(Event::ThemePreferenceSaved(result)).await;
                    },
                    Command::HistoryPage{requests,failures}=>{
                        request_page=requests.max(0);failure_page=failures.max(0);
                        history(&store,gateway.as_ref(),&sender,request_page,failure_page).await;
                    },
                    Command::RequestDetail(id)=>{
                        match store.request_record(&id).await {
                            Ok(Some(mut record))=>{
                                if let Some(g)=&gateway && let Some(live)=g.live_records().into_iter().find(|r|r.id==id){record=live;}
                                match store.failure_record(&id).await {
                                    Ok(failure)=>{let _=sender.try_send(Event::RequestDetail{record:Box::new(record),failure});},
                                    Err(_)=>{let _=sender.try_send(Event::Error("请求关联异常无法读取，请重试".into()));}
                                }
                            },
                            _=>{let _=sender.try_send(Event::Error("请求记录不存在、已过期或无法读取".into()));}
                        }
                    },
                    Command::RevealKey{id,copy}=>{
                        match store.reveal_key(&id).await {
                            Ok(value)=>{let _=sender.send(Event::KeyRevealed{key:CreatedKey{id,value},copy}).await;},
                            Err(e)=>{let _=sender.send(Event::Error(e.to_string())).await;},
                        }
                    },
                    Command::SelectAccount(id)=>{
                        if gateway.is_some() || !jobs.is_empty(){let _=sender.send(Event::Error("请先停止网关并等待当前操作结束，再切换账号".into())).await;continue;}
                        if service.activate(&id).await.is_err(){let _=sender.send(Event::Error("切换账号失败".into())).await;}
                        else {let _=sender.send(Event::Status{message:"已切换账号，已恢复其模型和测试记录".into(),busy:false}).await;}
                        snapshot(&store,&service,&sender).await;
                    },
                    Command::RefreshUsage(id)=>{
                        if usage_current.as_ref()!=Some(&id) && !usage_queue.contains(&id) && store.account(&id).await.is_ok_and(|v|v.is_some()){usage_queue.push_back(id);}
                    },
                    Command::CreateKey(label)=>{
                        match store.create_key(&label).await {Ok(key)=>{let _=sender.send(Event::KeyCreated(key)).await;},Err(_)=>{let _=sender.send(Event::Error("创建密钥失败：名称需为 1–80 个字符".into())).await;}}
                        snapshot(&store,&service,&sender).await;
                    },
                    Command::RevokeKey(id)=>{
                        if store.revoke_key(&id).await.is_err(){let _=sender.send(Event::Error("密钥撤销失败".into())).await;}
                        else {
                            if let Some(g)=&gateway {g.cancel_key(&id);}
                            let _=sender.send(Event::Status{message:"密钥已撤销并删除".into(),busy:!jobs.is_empty()}).await;
                        }
                        snapshot(&store,&service,&sender).await;
                    },
                    Command::Cancel=>operation.cancel(),
                    Command::StopGateway=>{
                        if let Some(mut g)=gateway.take(){g.stop().await;let _=sender.send(Event::Gateway{address:None}).await;}
                        history(&store,None,&sender,request_page,failure_page).await;
                        let _=sender.send(Event::Status{message:"网关已停止，监听及在途请求已关闭".into(),busy:!jobs.is_empty()}).await;
                    },
                    Command::StartGateway(value)=>{
                        if gateway.is_some(){continue;}
                        match start_gateway(&value,&store,&service).await {
                            Ok(g)=>{announce_gateway(&g,&sender,!jobs.is_empty()).await;gateway=Some(g);},
                            Err(e)=>{let _=sender.send(Event::Error(e)).await;}
                        }
                    },
                    command=>{
                        if !jobs.is_empty(){continue;}
                        if matches!(command,Command::SignIn) && gateway.is_some(){let _=sender.send(Event::Error("重新登录前请先停止网关".into())).await;continue;}
                        operation=shutdown.child_token();let cancel=operation.clone();let upstream=service.clone();let events=sender.clone();let local=store.clone();
                        let _=sender.send(Event::Status{message:"正在处理…".into(),busy:true}).await;
                        jobs.spawn(async move {
                            if let Command::TestModel(model)=command {
                                return test_and_save(&upstream,&local,&model,cancel).await.map(Outcome::Tested);
                            }
                            let work=async {
                                match command {
                                    Command::SignIn=>{
                                        let session=upstream.begin().await.map_err(|e|e.to_string())?;
                                        events.send(Event::DeviceCode{code:session.user_code.clone(),expires_in:session.expires_in}).await.map_err(|_|"窗口已关闭".to_owned())?;
                                        let _=events.send(Event::Status{message:"请在浏览器输入应用显示的设备码并授权。应用正在等待结果。".into(),busy:true}).await;
                                        let _=events.send(Event::OpenBrowser(Zeroizing::new(VERIFICATION_URL.into()))).await;
                                        upstream.finish_device(session,cancel.clone()).await.map_err(|e|e.to_string())?;
                                        Ok(Outcome::SignedIn)
                                    },
                                    Command::RefreshModels=>upstream.models().await.map(Outcome::Models).map_err(|e|e.to_string()),
                                    _=>unreachable!("local commands handled by owner"),
                                }
                            };
                            tokio::select! {biased;_=cancel.cancelled()=>Err("操作已取消".into()),r=work=>r}
                        });
                    }
                }
            },
            _=tick.tick()=>{updates.tick();history(&store,gateway.as_ref(),&sender,request_page,failure_page).await;}
        }
    }
    operation.cancel();
    jobs.abort_all();
    usage_jobs.abort_all();
    while jobs.join_next().await.is_some() {}
    if let Some(mut g) = gateway {
        g.stop().await;
    }
}
