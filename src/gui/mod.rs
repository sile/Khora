use std::convert::TryInto;

use eframe::{egui::{self, Label, Output, Sense}, epi};
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
    send_amount: Vec<String>,

    fee: String,

    unstaked: String,

    staked: String,

    friends: Vec<String>,

    friend_adding: String,

    name_adding: String,

    friend_names: Vec<String>,

    staking: bool,

    stake: String,

    unstake: String,

    addr: String,

    stkaddr: String,

    edit_names: Vec<bool>,
}
impl Default for TemplateApp {
    fn default() -> Self {
        let (_,r) = channel::bounded::<Vec<u8>>(0);
        let (s,_) = mpsc::channel::<Vec<u8>>();
        TemplateApp{
            send_amount: vec![],
            stake: "0".to_string(),
            unstake: "0".to_string(),
            fee: "0".to_string(),
            reciever: r,
            sender: s,
            unstaked: "0".to_string(),
            staked: "0".to_string(),
            friends: vec![],
            edit_names: vec![],
            friend_names: vec![],
            friend_adding: "".to_string(),
            name_adding: "".to_string(),
            staking: false,
            addr: "".to_string(),
            stkaddr: "".to_string(),
        }
    }
}
impl TemplateApp {
    pub fn new_minimal(reciever: channel::Receiver<Vec<u8>>, sender: mpsc::Sender<Vec<u8>>) -> Self {
        TemplateApp{reciever, sender, ..Default::default()}
    }
    pub fn new(reciever: channel::Receiver<Vec<u8>>, sender: mpsc::Sender<Vec<u8>>, staked: String, addr: String, stkaddr: String) -> Self {
        TemplateApp{
            reciever,
            sender,
            staked,
            addr,
            stkaddr,
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
                self.unstaked = format!("{}",u64::from_le_bytes(u.try_into().unwrap()));
                self.staked = format!("{}",u64::from_le_bytes(i.try_into().unwrap()));
            }
        }

        let Self {
            send_amount,
            fee,
            reciever: _,
            sender,
            unstaked,
            staked,
            friends,
            edit_names,
            friend_names,
            friend_adding,
            name_adding,
            staking,
            stake,
            unstake,
            addr,
            stkaddr,
        } = self;

 

        // // Examples of how to create different panels and windows.
        // // Pick whichever suits you.
        // // Tip: a good default choice is to just keep the `CentralPanel`.
        // // For inspiration and more examples, go to https://emilk.github.io/egui

        // egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
        //     // The top panel is often a good place for a menu bar:
        //     egui::menu::bar(ui, |ui| {
        //         egui::menu::menu(ui, "File", |ui| {
        //             if ui.button("Quit").clicked() {
        //                 frame.quit();
        //             }
        //         });
        //     });
        //     // egui::util::undoer::default(); // there's some undo button
        // });

        egui::CentralPanel::default().show(ctx, |ui| { // the only option for staker stuff should be to send x of money to self (starting with smallest accs)
            // The central panel the region left after adding TopPanel's and SidePanel's

            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                });
            });
            ui.heading("Kora");
            ui.hyperlink("https://github.com/constantine1024/Kora");
            ui.add(egui::github_link_file!(
                "https://github.com/constantine1024/Kora",
                "Source code."
            ));
            ui.horizontal(|ui| {
                if ui.button("📋").on_hover_text("Click to copy the address to clipboard").clicked() {
                    ui.output().copied_text = addr.clone();
                }
                if ui.add(Label::new("address").sense(Sense::hover())).hovered() {
                    ui.small(&*addr);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Unstaked Money");
                ui.label(&*unstaked);
            });
            ui.horizontal(|ui| {
                ui.label("Staked Money ");
                ui.label(&*staked);
                // let bytes: Vec<_> = s.bytes().rev().collect(); // something like this maybe? (would need to handle tx differently though)
                // let chunks: Vec<_> = bytes.chunks(3).map(|chunk| str::from_utf8(chunk).unwrap()).collect();
                // let result: Vec<_> = chunks.connect(" ").bytes().rev().collect();
                // String::from_utf8(result).unwrap()
            });
            // ui.horizontal(|ui| {
            //     ui.label("Maximum Money");
            //     ui.label(format!("{}",2u64.pow(41)));
            // });

            ui.horizontal(|ui| {
                ui.text_edit_singleline(stake);
                if ui.button("Stake").clicked() {
                    let mut m = vec![];
                    m.extend(stkaddr.as_bytes().to_vec());
                    m.extend(stake.parse::<u64>().unwrap().to_le_bytes().to_vec());
                    println!("-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*\n{},{},{}",unstaked,fee,stake);
                    let x = unstaked.parse::<u64>().unwrap() - fee.parse::<u64>().unwrap() - stake.parse::<u64>().unwrap();
                    if x > 0 {
                        m.extend(addr.as_bytes().to_vec());
                        m.extend(x.to_le_bytes().to_vec());
                    }
                    m.push(33);
                    m.push(33);
                    sender.send(m).expect("something's wrong with communication from the gui");
                }
            });
            ui.horizontal(|ui| {
                ui.text_edit_singleline(unstake);
                if ui.button("Unstake").clicked() {
                    // println!("unstaking {:?}!",unstake.parse::<u64>());
                    let mut m = vec![];
                    m.extend(addr.as_bytes().to_vec());
                    m.extend(unstake.parse::<u64>().unwrap().to_le_bytes());
                    // println!("-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*\n{},{},{}",staked,fee,unstake);
                    let x = staked.parse::<u64>().unwrap() - fee.parse::<u64>().unwrap() - unstake.parse::<u64>().unwrap();
                    if x > 0 {
                        m.extend(stkaddr.as_bytes());
                        m.extend(x.to_le_bytes());
                    }
                    m.push(63);
                    m.push(33);
                    println!("{}",String::from_utf8_lossy(&m));
                    sender.send(m).expect("something's wrong with communication from the gui");
                }
            });
            ui.label("Transaction Fee:");
            ui.text_edit_singleline(fee);
            egui::warn_if_debug_build(ui);
        });




        egui::SidePanel::right("Right Panel").show(ctx, |ui| {
            egui::ScrollArea::auto_sized().show(ui,|ui| {
                ui.heading("Friends");
                ui.label("Add Friend:");
                ui.horizontal(|ui| {
                    ui.small("name");
                    ui.text_edit_singleline(name_adding);
                });
                ui.horizontal(|ui| {
                    ui.small("address");
                    ui.text_edit_singleline(friend_adding);
                });
                if ui.button("Add Friend").clicked() {
                    friends.push(friend_adding.clone());
                    friend_names.push(name_adding.clone());
                    edit_names.push(false);
                    send_amount.push("0".to_string());
                    *friend_adding = "".to_string();
                    *name_adding = "".to_string();
                }
                let mut friend_deleted = usize::MAX;
                ui.label("Friends: ");
                for ((i,((addr,name),amnt)),e) in friends.iter_mut().zip(friend_names.iter_mut()).zip(send_amount.iter_mut()).enumerate().zip(edit_names.iter_mut()) {
                    if ui.button("edit").clicked() {
                        *e = !*e;
                    }
                    if *e {
                        ui.text_edit_singleline(name);
                        ui.text_edit_singleline(addr);
                    } else {
                        ui.label(&*name);
                        ui.small(&*addr);
                    }
                    ui.horizontal(|ui| {
                        if *e {
                            if ui.button("Delete Friend").clicked() {
                                friend_deleted = i;
                            }
                        } else {
                            ui.label("Send:");
                            ui.text_edit_singleline(amnt);
                        }
                    });
                }
                if friend_deleted != usize::MAX {
                    friend_names.remove(friend_deleted);
                    friends.remove(friend_deleted);
                    send_amount.remove(friend_deleted);
                }
                ui.horizontal(|ui| {
                    if ui.button("Clear Transaction").clicked() {
                        send_amount.iter_mut().for_each(|x| *x = "0".to_string());
                    }
                    if ui.button("Send Transaction").clicked() {
                        let mut m = vec![];
                        let mut tot = 0u64;
                        for (who,amnt) in friends.iter_mut().zip(send_amount.iter_mut()) {
                            let x = amnt.parse::<u64>().unwrap();
                            if x > 0 {
                                m.extend(str::to_ascii_lowercase(&who).as_bytes().to_vec());
                                m.extend(x.to_le_bytes().to_vec());
                                tot += x;
                            }
                        }
                        let x = unstaked.parse::<u64>().unwrap() - tot - fee.parse::<u64>().unwrap();
                        if x > 0 {
                            m.extend(str::to_ascii_lowercase(&addr).as_bytes());
                            m.extend(x.to_le_bytes());
                        }
                        m.push(33);
                        m.push(33);
                        sender.send(m).expect("something's wrong with communication from the gui");
                    }
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Hyperlink::new("https://github.com/emilk/egui/").text("powered by egui"),
                    );
                });
            });
        });
    
    }
}
