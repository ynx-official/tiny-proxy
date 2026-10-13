use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

struct Probe;
impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_4()
            .child("Tiny Proxy · Windows 技术验证")
            .child("此窗口仅验证 GPUI Kit，不代表产品界面或登录成功。")
            .child(
                Button::new("quit")
                    .label("关闭验证窗口")
                    .on_click(|_, _, cx| cx.quit()),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Probe))
                .expect("GPUI probe window could not be opened");
            cx.activate(true);
        });
}
