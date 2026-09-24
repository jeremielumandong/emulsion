use gpui_kit::component::{Root, button::Button, menu::DropdownMenu};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    Bounds, Context, Point, ScrollDelta, TestAppContext, Window, div, point, prelude::*, px, size,
};

struct Menus;
impl Render for Menus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                Button::new("commands")
                    .label("Commands")
                    .dropdown_menu(|menu, window, cx| {
                        (0..40)
                            .fold(menu.min_w(px(400.)).max_h(px(900.)), |menu, i| {
                                menu.menu(format!("Command {i}"), Box::new(crate::actions::Save))
                            })
                            .submenu("More", window, cx, |menu, _, _| {
                                (0..40).fold(menu, |menu, i| {
                                    menu.menu(format!("Nested {i}"), Box::new(crate::actions::Save))
                                })
                            })
                    }),
            )
    }
}

#[gpui_kit::test]
fn popup_scrolls_and_fits_after_resize(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let menus = cx.new(|_| Menus);
        Root::new(menus, window, cx)
    });
    cx.simulate_resize(size(px(640.), px(700.)));
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.click("commands", cx);
    });
    cx.simulate_resize(size(px(280.), px(240.)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        let menu = window.find("popup-menu");
        let viewport = Bounds::new(Point::default(), window.viewport_size());
        assert!(viewport.contains(&menu.bounds().origin));
        assert!(
            viewport.contains(&menu.bounds().bottom_right()),
            "{:?}",
            menu.bounds()
        );
        assert!(!window.within("popup-menu").find(40usize).visible());
        window.scroll(
            "popup-menu",
            ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            cx,
        );
        assert!(window.within("popup-menu").find(40usize).visible());
        window.press("escape", cx);
        window.click("commands", cx);
        // Wrap to the last row using only the keyboard, then enter its submenu.
        window.press("up", cx);
        // In this narrow window the submenu opens to the left.
        window.press("left", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        let submenu = window.find("submenu");
        let viewport = Bounds::new(Point::default(), window.viewport_size());
        assert!(viewport.contains(&submenu.bounds().origin));
        assert!(viewport.contains(&submenu.bounds().bottom_right()));
        window.within("submenu").press("up", cx);
        assert!(window.within("submenu").find(39usize).visible());
    });
}
