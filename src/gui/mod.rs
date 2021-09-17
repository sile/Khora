use std::convert::TryInto;

use eframe::{egui, epi};
use crossbeam::channel;
use fibers::sync::mpsc;

/*
cargo run --bin full_staker --release 9876 pig
cargo run --bin full_staker --release 9877 dog 0 9876
cargo run --bin full_staker --release 9878 cow 0 9876
cargo run --bin full_staker --release 9879 ant 0 9876
*/
/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[cfg_attr(feature = "persistence", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "persistence", serde(default))] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for reciever i think
    reciever: channel::Receiver<Vec<u8>>,

    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for reciever i think
    sender: mpsc::Sender<Vec<u8>>,

    // Example stuff:
    send_amount: String,

    tx_recipient: String,

    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))]
    value: f32,

    info: String,

    unstaked: String,

    staked: String,

    friends: Vec<String>,

    friend_adding: String,
}
impl Default for TemplateApp {
    fn default() -> Self {
        let (_,r) = channel::bounded::<Vec<u8>>(0);
        let (s,_) = mpsc::channel::<Vec<u8>>();
        TemplateApp{
            send_amount: "".to_string(),
            tx_recipient: "".to_string(),
            value: 1.2,
            reciever: r,
            sender: s,
            info: "hi newbee!".to_string(),
            unstaked: "0".to_string(),
            staked: "0".to_string(),
            friends: vec![],
            friend_adding: "add_friends_here!".to_string(),
        }
    }
}
impl TemplateApp {
    pub fn new_minimal(reciever: channel::Receiver<Vec<u8>>, sender: mpsc::Sender<Vec<u8>>) -> Self {
        TemplateApp{reciever, sender, ..Default::default()}
    }
    pub fn new(reciever: channel::Receiver<Vec<u8>>, sender: mpsc::Sender<Vec<u8>>, info: String) -> Self {
        TemplateApp{
            reciever,
            sender,
            info,
            ..Default::default()
        }
    }
}
impl epi::App for TemplateApp {
    fn name(&self) -> &str {
        "Kora" // saved as ~/.local/share/kora
    }

    /// Called once before the first frame.
    fn setup(
        &mut self,
        _ctx: &egui::CtxRef,
        _frame: &mut epi::Frame<'_>,
        _storage: Option<&dyn epi::Storage>,
    ) {
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        #[cfg(feature = "persistence")]
        if let Some(storage) = _storage {
            *self = epi::get_value(storage, epi::APP_KEY).unwrap_or_default()
        }
    }

    /// Called by the frame work to save state before shutdown.
    /// Note that you must enable the `persistence` feature for this to work.
    #[cfg(feature = "persistence")]
    fn save(&mut self, storage: &mut dyn epi::Storage) {
        epi::set_value(storage, epi::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        if let Ok(mut i) = self.reciever.try_recv() {
            let modify = i.pop().unwrap();
            if modify == 0 {
                let u = i.drain(..8).collect::<Vec<_>>();
                self.unstaked = format!("unstaked: {}",u64::from_le_bytes(u.try_into().unwrap()));
                self.staked = format!("staked: {}",u64::from_le_bytes(i.try_into().unwrap()));
            }
        }

        let Self { send_amount, tx_recipient, value, reciever: _, sender, info, unstaked, staked, friends, friend_adding } = self;

 

        // Examples of how to create different panels and windows.
        // Pick whichever suits you.
        // Tip: a good default choice is to just keep the `CentralPanel`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                });
            });
        });

        egui::SidePanel::left("side_panel").show(ctx, |ui| {
            ui.heading("Side Panel");
            ui.label(&*info);
            ui.label(&*unstaked);
            ui.label(&*staked);
            ui.add(egui::Slider::new(value, 0.0..=10.0).text("value"));
            if ui.button("Increment").clicked() {
                *value += 1.0;
            }
            if ui.button("print info in terminal").clicked() { // this is how you send info to the program. fill vec with different stuff depending on what you want
                sender.send(vec![]);
            }

            ui.horizontal(|ui| {
                ui.label("Reciever: ");
                ui.text_edit_singleline(tx_recipient);
                ui.label("Amount: ");
                ui.text_edit_singleline(send_amount);
            });
            ui.horizontal(|ui| {
                ui.label("Add Friend: ");
                ui.text_edit_singleline(friend_adding);
                if ui.button("Add Friend and Clear Blank Friends").clicked() {
                    friends.push(friend_adding.clone());
                    friends.retain(|x| !x.is_empty());
                    *friend_adding = "".to_string();
                }
            });
            ui.label("Friends: ");
            for f in friends {
                ui.text_edit_singleline(f);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add(
                    egui::Hyperlink::new("https://github.com/emilk/egui/").text("powered by egui"),
                );
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            ui.heading("egui template");
            ui.hyperlink("https://github.com/emilk/egui_template");
            ui.add(egui::github_link_file!(
                "https://github.com/emilk/egui_template/blob/master/",
                "Source code."
            ));
            // ui.label(&*info);
            egui::warn_if_debug_build(ui);
        });

        if false {
            egui::Window::new("Window").show(ctx, |ui| {
                ui.label("Windows can be moved by dragging them.");
                ui.label("They are automatically sized based on contents.");
                ui.label("You can turn on resizing and scrolling if you like.");
                ui.label("You would normally chose either panels OR windows.");
            });
        }
    }
}
