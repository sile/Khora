use std::{sync::mpsc::{self, Sender, Receiver}, thread};
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use fibers_global::handle;
use futures::{Async, Future, Poll, task};
use macroquad::prelude::*;
use tokio::{runtime::{Handle, Runtime}, select, sync::mpsc::channel};

#[derive(Debug)]
pub struct View {
    button_label: String,
    sender: Sender<i32>,
    reciever: Receiver<String>,
}

impl View {
    fn new(s: Sender<i32>, r: Receiver<String>) -> Self {
        View{
            button_label: "0".to_string(),
            sender: s,
            reciever: r,
        }
    }

    async fn run_frame_async(&mut self) {

        // Runtime::new().unwrap().block_on( async {
        clear_background(PINK);

        if let Ok(x) = self.reciever.try_recv() {
            self.button_label = x;
        }
        // Process keys, mouse etc.

        egui_macroquad::ui(|egui_ctx| {
            egui::Window::new("egui ❤ macroquad")
                .show(egui_ctx, |ui| {
                    ui.label("Something");
                    ui.horizontal(|ui| {
                        if ui.button("-").clicked() {
                            self.sender.send(-1).unwrap();
                            println!("click!");
                        }
                        if ui.button("+").clicked() {
                            self.sender.send(1).unwrap();
                            println!("clonk!");
                        }
                        if ui.button("0").clicked() {
                            self.sender.send(-self.button_label.parse::<i32>().unwrap()).unwrap();
                            println!("boop!");
                        }
                        ui.label(&*self.button_label);
                    });
                });
            // egui::CentralPanel::default()
            //     .show(egui_ctx, |ui| {
            //         ui.label("Something Else");
            //         ui.horizontal(|ui| {
            //             if ui.button("-").clicked() {
            //                 self.sender.send(1).unwrap();
            //                 println!("click!");
            //             }
            //             ui.label(&*self.button_label);
            //             if ui.button("+").clicked() {
            //                 self.sender.send(1).unwrap();
            //                 println!("clonk!");
            //             }
            //         });
            //     });
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
    let mut view = View::new(s,rback);
    loop {view.run_frame_async().await;}
}
