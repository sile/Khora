use std::{sync::mpsc, thread};

use macroquad::prelude::*;


fn ui_counter(ui: &mut egui::Ui, counter: &mut i32) {
    // Put the buttons and label on the same row:
    ui.horizontal(|ui| {
        if ui.button("-").clicked() {
            *counter -= 1;
        }
        ui.label(counter.to_string());
        if ui.button("+").clicked() {
            *counter += 1;
        }
    });
}

#[macroquad::main("egui with macroquad")]
async fn main() {

    let (s,r) = mpsc::channel::<i32>();

    thread::spawn(move || {
        let mut amnt = 0;
        while let Ok(x) = r.recv() {
            amnt += x;
            println!("{}",amnt);
        };
    });
    loop {
        clear_background(WHITE);

        // Process keys, mouse etc.

        egui_macroquad::ui(|egui_ctx| {
            egui::Window::new("egui ❤ macroquad")
                .show(egui_ctx, |ui| {
                    ui.label("Test");
                    ui.horizontal(|ui| {
                        if ui.button("-").clicked() {
                            s.send(-1).unwrap();
                            println!("click!");
                        }
                        if ui.button("+").clicked() {
                            s.send(1).unwrap();
                            println!("clonk!");
                        }
                    });
                });
        });

        // Draw things before egui

        egui_macroquad::draw();
        
        // Draw things after egui

        next_frame().await;
    }
}
