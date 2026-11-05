#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod camera;
mod receiver;
mod theme;
mod ui;
fn main() {
    if std::env::args().any(|a| a == "--self-test") {
        match receiver::self_test() {
            Ok(v) => println!("{v}"),
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Err(e) = ui::run() {
        ui::error(&format!("{e:#}"));
    }
}
