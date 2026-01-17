use eframe::egui;

macro_rules! imgpath {
    ($p:literal) => {
        concat!(env!("CARGO_MANIFEST_DIR"), "/img/", $p)
    };
}

pub const COW: egui::ImageSource<'static> = egui::include_image!(imgpath!("cow.png"));
pub const DRUM: egui::ImageSource<'static> = egui::include_image!(imgpath!("drum.png"));
pub const MIC: egui::ImageSource<'static> = egui::include_image!(imgpath!("mic.png"));
pub const SAXO: egui::ImageSource<'static> = egui::include_image!(imgpath!("saxo.png"));
pub const FISH: egui::ImageSource<'static> = egui::include_image!(imgpath!("fish.png"));
pub const X: egui::ImageSource<'static> = egui::include_image!(imgpath!("x.png"));
