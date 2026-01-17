use eframe::egui;

pub trait ImageExt {
    fn hflip(self) -> Self;
}

impl ImageExt for egui::Image<'_> {
    fn hflip(self) -> Self {
        self.uv(egui::Rect::from_min_max(
            egui::pos2(1.0, 0.0),
            egui::pos2(0.0, 1.0),
        ))
    }
}

#[allow(unused_macros)]
macro_rules! edbg {
    ($ctx:expr, $($val:expr),+ $(,)?) => {
        let ctx: &egui::Context = $ctx;
        // Empty line to work around mouse pointer overlapping with text
        ctx.debug_text("");
        $(
            ctx.debug_text(format!("{}: {:?}", stringify!($val), $val));
        )+
    }
}

#[allow(unused_imports)]
pub(crate) use edbg;
