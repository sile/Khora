use std::{sync::mpsc::{self, Sender, Receiver}, thread};
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use macroquad::prelude::*;
use serde::{Serialize, Deserialize};


#[derive(Default, Clone, Serialize, Deserialize, Hash, Debug)]
pub struct View {
    button_label: String,
}

impl View {
    async fn run_window(&mut self, s: &Sender<i32>, r: &Receiver<String>) {
        clear_background(PINK);

        if let Ok(x) = r.try_recv() {
            self.button_label = x;
        }
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
                        ui.label(&*self.button_label);
                        if ui.button("+").clicked() {
                            s.send(1).unwrap();
                            println!("clonk!");
                        }
                    });
                });
            egui::CentralPanel::default()
                .show(egui_ctx, |ui| {
                    ui.label("Test");
                    ui.horizontal(|ui| {
                        if ui.button("-").clicked() {
                            s.send(1).unwrap();
                            println!("click!");
                        }
                        ui.label(&*self.button_label);
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
#[macroquad::main("egui with macroquad")]
async fn main() {

    let (s,r) = mpsc::channel::<i32>();
    let (sback,rback) = mpsc::channel::<String>();

    thread::spawn(move ||  {
        println!("hellow from the thread!");
        let mut amnt = 0;
        while let Ok(x) = r.recv() {
            amnt += x;
            println!("{}",amnt);
            sback.send(format!("{}",amnt)).unwrap();
        };
    });
    let mut view = View::default();


    let executor = ThreadPoolExecutor::new().unwrap();
    executor.spawn(futures::lazy(|| {
        loop {
            view.run_window(&s, &rback).await;
        };
    }));
    executor.run().unwrap();
}
