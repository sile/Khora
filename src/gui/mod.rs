use std::{convert::TryInto, fs, time::Instant};

use curve25519_dalek::scalar::Scalar;
use eframe::{egui::{self, Checkbox, Label, Sense}, epi};
use crossbeam::channel;
use fibers::sync::mpsc;

use getrandom::getrandom;
use sha3::{Digest, Sha3_512};

/*
cargo run --bin full_staker --release 9876 pig
cargo run --bin full_staker --release 9877 dog 0 9876
cargo run --bin full_staker --release 9878 cow 0 9876
cargo run --bin full_staker --release 9879 ant 0 9876
*/


fn random_pswrd() -> String {
    let mut chars = vec![0u8;40];
    loop {
        getrandom(&mut chars).expect("something's wrong with your randomness");
        chars = chars.into_iter().filter(|x| *x < 248).take(20).collect();
        if chars.len() == 20 {
            break
        }
    }
    chars.iter_mut().for_each(|x| {
        *x %= 62;
        *x += 48;
        if *x > 57 {
            *x += 7
        }
        if *x > 90 {
            *x += 6;
        }
    });
    chars.into_iter().map(char::from).collect()
}
fn get_pswrd(a: &String, b: &String, c: &String) -> Vec<u8> {
    println!("{}",a);
    println!("{}",b);
    println!("{}",c);
    let mut hasher = Sha3_512::new();
    hasher.update(&a.as_bytes());
    hasher.update(&b.as_bytes());
    hasher.update(&c.as_bytes());
    Scalar::from_hash(hasher).as_bytes().to_vec()
}
/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[cfg_attr(feature = "persistence", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "persistence", serde(default))] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for reciever
    reciever: channel::Receiver<Vec<u8>>,

    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for sender
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
    dont_trust_amounts: bool,
    password0: String,
    pswd_guess0: String,
    username: String,
    secret_key: String,
    pswd_shown: bool,
    block_number: u64,
    show_next_pswrd: bool,
    next_pswrd0: String,
    next_pswrd1: String,
    next_pswrd2: String,
    panic_fee: String,
    entrypoint: String,
    stkspeand: bool,
    show_reset: bool,
    setup: bool,
    eta: i8,

    #[cfg_attr(feature = "persistence", serde(skip))] // this feature doesn't work for sender
    timekeeper: Instant,

    // this how you opt-out of serialization of a member
    #[cfg_attr(feature = "persistence", serde(skip))]
    you_cant_do_that: bool,
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
            dont_trust_amounts: false,
            password0: "".to_string(),
            pswd_guess0: "".to_string(),
            username: "".to_string(),
            secret_key: "".to_string(),
            pswd_shown: true,
            block_number: 0,
            show_next_pswrd: true,
            next_pswrd0: random_pswrd(),
            next_pswrd1: random_pswrd(),
            next_pswrd2: random_pswrd()[..5].to_string(),
            panic_fee: "1".to_string(),
            entrypoint: "".to_string(),
            stkspeand: false,
            show_reset: false,
            you_cant_do_that: false,
            eta: 60,
            timekeeper: Instant::now(),
            setup: false,
        }
    }
}
impl TemplateApp {
    pub fn new_minimal(reciever: channel::Receiver<Vec<u8>>, sender: mpsc::Sender<Vec<u8>>) -> Self {
        TemplateApp{reciever, sender, ..Default::default()}
    }
    pub fn new(reciever: channel::Receiver<Vec<u8>>, sender: mpsc::Sender<Vec<u8>>, addr: String, stkaddr: String, setup: bool) -> Self {
        TemplateApp{
            reciever,
            sender,
            addr,
            stkaddr,
            setup,
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
        println!("This is printing before the first frame!");
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if !self.setup {
            println!("Attempting to load app state");
            #[cfg(feature = "persistence")]
            if let Some(storage) = _storage {
                let r = self.reciever.clone();
                let s = self.sender.clone();
                let a = self.addr.clone();
                let sa = self.stkaddr.clone();
                *self = epi::get_value(storage, epi::APP_KEY).unwrap_or_default();
                self.sender = s;
                self.reciever = r;
                self.addr = a;
                self.stkaddr = sa;
            }
        } else {
            self.secret_key = random_pswrd()[..5].to_string();
        }
    }

    /// Called by the frame work to save state before shutdown.
    /// Note that you must enable the `persistence` feature for this to work.
    #[cfg(feature = "persistence")]
    fn save(&mut self, storage: &mut dyn epi::Storage) {
        println!("App closing procedures beginning...");
        if self.setup {
            println!("Setting password...");
            self.password0 = self.pswd_guess0.clone();
            loop {
                if self.sender.send(get_pswrd(&self.password0,&self.username,&self.secret_key)).is_ok() {
                    break
                }
            }
            loop {
                if self.sender.send(vec![(self.friend_names.len() == 0) as u8]).is_ok() {
                    break
                }
            }
        }
        self.setup = false;
        epi::set_value(storage, epi::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        if let Ok(mut i) = self.reciever.try_recv() {
            let modification = i.pop().unwrap();
            if modification == 0 {
                let u = i.drain(..8).collect::<Vec<_>>();
                self.unstaked = format!("{}",u64::from_le_bytes(u.try_into().unwrap()));
                self.staked = format!("{}",u64::from_le_bytes(i.try_into().unwrap()));
            } else if modification == 1 {
                self.dont_trust_amounts = i.pop() == Some(0);
            } else if modification == 2 {
                self.block_number = u64::from_le_bytes(i.try_into().unwrap());
            } else if modification == 128 {
                self.eta = i[0] as i8;
                self.timekeeper = Instant::now();
            } else if modification == u8::MAX {
                if i.pop() == Some(0) {
                    self.addr = String::from_utf8_lossy(&i).to_string();
                } else {
                    self.stkaddr = String::from_utf8_lossy(&i).to_string();
                }
            }
            self.staking = self.staked != "0".to_string();
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
            dont_trust_amounts,
            password0,
            pswd_guess0,
            username,
            secret_key,
            eta,
            timekeeper,
            pswd_shown,
            block_number,
            show_next_pswrd,
            next_pswrd0,
            next_pswrd1,
            next_pswrd2,
            panic_fee,
            entrypoint,
            stkspeand,
            show_reset,
            you_cant_do_that,
            setup,
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
                    if ui.button("Enter Network").clicked() && !*setup {
                        let mut m = entrypoint.as_bytes().to_vec();
                        m.push(42);
                        sender.send(m).expect("something's wrong with communication from the gui");
                    }
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                    if ui.button("Panic Options").clicked() {
                        *show_reset = !*show_reset;
                    }
                    if ui.button("Go To Setup").clicked() {
                        fs::remove_file("myNode");
                        frame.quit();
                    }
                });
                ui.label("entry address");
                ui.text_edit_singleline(entrypoint);
            });
            ui.heading("Kora");
            ui.hyperlink("https://khora.info");
            ui.add(egui::github_link_file!(
                "https://github.com/constantine1024/Kora",
                "Source code."
            ));
            ui.label(format!("current block: {}",block_number));
            ui.horizontal(|ui| {
                ui.label("next block in");
                ui.add(Label::new(format!("{}",*eta - timekeeper.elapsed().as_secs() as i8)).strong().text_color(egui::Color32::YELLOW));
            });
            ui.horizontal(|ui| {
                if ui.button("📋").on_hover_text("Click to copy the address to clipboard").clicked() {
                    ui.output().copied_text = addr.clone();
                }
                if ui.add(Label::new("address").sense(Sense::hover())).hovered() {
                    ui.small(&*addr);
                }
            });
            if *staking {
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy the address to clipboard").clicked() {
                        ui.output().copied_text = stkaddr.clone();
                    }
                    if ui.add(Label::new("staking address").sense(Sense::hover())).hovered() {
                        ui.small(&*stkaddr);
                    }
                });
            }

            ui.horizontal(|ui| {
                ui.label("Unstaked Money");
                ui.label(&*unstaked);
            });
            if *staking {
                ui.horizontal(|ui| {
                    ui.label("Staked Money ");
                    ui.label(&*staked);
                });
            }

            ui.horizontal(|ui| {
                ui.text_edit_singleline(stake);
                if pswd_guess0 == password0 {
                    if ui.button("Stake").clicked() && !*setup {
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
                }
            });
            if *staking {
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(unstake);
                    if pswd_guess0 == password0 {
                        if ui.button("Unstake").clicked() && !*setup {
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
                    }
                });
            }
            ui.label("Transaction Fee:");
            ui.text_edit_singleline(fee);
            ui.horizontal(|ui| {
                if ui.button("sync").clicked() && !*setup {
                    sender.send(vec![121]).expect("something's wrong with communication from the gui");
                }
                if ui.button("toggle password").clicked() {
                    *pswd_shown = !*pswd_shown;
                }
            });
            if *pswd_shown {
                ui.text_edit_singleline(pswd_guess0);
            }
            if *setup {
                ui.text_edit_singleline(username);
                ui.text_edit_singleline(secret_key);
            } else {
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy the address to clipboard").clicked() {
                        ui.output().copied_text = username.clone();
                    }
                    ui.label(&*username);
                });
                ui.horizontal(|ui| {
                    if ui.button("📋").on_hover_text("Click to copy the address to clipboard").clicked() {
                        ui.output().copied_text = secret_key.clone();
                    }
                    ui.label(&*secret_key);
                });
            }
            if *setup {
                ui.add(Label::new("Welcome to Khora! Type your password into the password box then turn me off to create your wallet!\nIf you are planning on being a staker, you need to save the history... Add a friend to do so!").text_color(egui::Color32::RED));
            } else if pswd_guess0 != password0 {
                ui.add(Label::new("password incorrect, features disabled").text_color(egui::Color32::RED));
            }
            if *dont_trust_amounts {
                ui.add(Label::new("money owned is not yet verified").text_color(egui::Color32::RED));
            }
            if *you_cant_do_that {
                if ui.add(Label::new("you don't have the funds to make that transaction").text_color(egui::Color32::RED).sense(Sense::hover())).hovered() {
                    *you_cant_do_that = false;
                }
            }
            egui::warn_if_debug_build(ui);
        });

        if  pswd_guess0 == password0 || *setup { // add warning to not panic 2ce in a row
            egui::Window::new("Reset Options").open(show_reset).show(ctx, |ui| {
                if ui.add(Label::new("Panic Button").heading().sense(Sense::hover())).hovered() {
                    ui.small("Password reset: 'panic button' changes your password and sends all of your money to a new account. If you click this button do not turn off this app until receiving confirmation or your account balance may be lost. After receiving 1 transaction (2 for stakers), the old account information will be deleted.");
                }
                ui.horizontal(|ui| {
                    ui.add(Checkbox::new(show_next_pswrd,"Show Password On Reset"));
                    if ui.button("Suggest Password").clicked() {
                        *next_pswrd0 = random_pswrd();
                        *next_pswrd1 = random_pswrd();
                        *next_pswrd2 = random_pswrd()[..5].to_string();
                    }
                });
                ui.label("Next Passwords");
                ui.text_edit_singleline(next_pswrd0);
                ui.text_edit_singleline(next_pswrd1);
                ui.label(&*next_pswrd2);
                ui.horizontal(|ui| {
                    ui.label("Password Reset Fee");
                    ui.text_edit_singleline(panic_fee);
                });
                
                if ui.button("Reset").clicked() {
                    let mut x = vec![];
                    let pf = panic_fee.parse::<u64>().unwrap();

                    let s = unstaked.parse::<u64>().unwrap();
                    if s > pf {
                        x.extend((s - pf).to_le_bytes());
                    } else {
                        x.extend(s.to_le_bytes());
                    }
                    let s = staked.parse::<u64>().unwrap();
                    if s > pf {
                        x.extend((s - pf).to_le_bytes());
                    } else {
                        x.extend(s.to_le_bytes());
                    }
                    x.extend(get_pswrd(&*next_pswrd0,&*next_pswrd1,&*next_pswrd2));
                    x.push(u8::MAX);
                    if !*setup {
                        sender.send(x).expect("something's wrong with communication from the gui");
                    }
                    *password0 = next_pswrd0.clone();
                    *username = next_pswrd1.clone();
                    *secret_key = next_pswrd2.clone();
                    if *show_next_pswrd {
                        *pswd_guess0 = next_pswrd0.clone();
                    }
                }
            });
        }
        
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
                    if *e {
                        ui.text_edit_singleline(name);
                        ui.text_edit_singleline(addr);
                    } else {
                        ui.label(&*name);
                        ui.small(&*addr);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("edit").clicked() {
                            *e = !*e;
                        }
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
                    if pswd_guess0 == password0 {
                        if ui.button("Send Transaction").clicked() && !*setup {
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
                            if *stkspeand {
                                let x = staked.parse::<u64>().unwrap() - tot - fee.parse::<u64>().unwrap();
                                if x > 0 {
                                    m.extend(str::to_ascii_lowercase(&stkaddr).as_bytes());
                                    m.extend(x.to_le_bytes());
                                }
                                m.push(63);
                                *you_cant_do_that = staked.parse::<u64>().unwrap() < tot + fee.parse::<u64>().unwrap();
                            } else {
                                let x = unstaked.parse::<u64>().unwrap() - tot - fee.parse::<u64>().unwrap();
                                if x > 0 {
                                    m.extend(str::to_ascii_lowercase(&stkaddr).as_bytes());
                                    m.extend(x.to_le_bytes());
                                }
                                m.push(33);
                                *you_cant_do_that = unstaked.parse::<u64>().unwrap() < tot + fee.parse::<u64>().unwrap();
                            }
                            if !*you_cant_do_that {
                                m.push(33);
                                sender.send(m).expect("something's wrong with communication from the gui");
                            }
                        }
                    }
                    if *staking {
                        ui.add(Checkbox::new(stkspeand,"Spend with staked money"));
                    }
                });
            });
        });
    
    }
}
