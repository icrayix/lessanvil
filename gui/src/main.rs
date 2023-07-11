#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{path::PathBuf, sync::mpsc, fs};

use eframe::{egui, glow::CONTEXT_RELEASE_BEHAVIOR};
use fs_extra::dir::CopyOptions;
use lessanvil::{Config, ProcessingUpdate, Error};

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        // drag_and_drop_support: true,
        initial_window_size: Some(egui::vec2(320.0, 240.0)),
        ..Default::default()
    };
    eframe::run_native(
        "Lessanvil GUI",
        options,
        Box::new(|_cc| Box::<MyApp>::default()),
    )
}

#[derive(Default)]
struct MyApp {
    world_path: Option<String>,
    backup_path: Option<String>,
    max_inhabited_time_str: String,
    thread_count_str: String,
    accept: bool,
    max_inhabited_time: usize,
    thread_count: usize,
    errs: Vec<String>,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // self.errs.clear();
/*
            world_path: Option<String>;
            backup_path: Option<String>;
            max_inhabited_time: String;
            thread_count: String;
*/

            ui.horizontal(|ui| {
                ui.label("Maximum time players were on a chunk to be deleted");
                if ui
                    .text_edit_singleline(&mut self.max_inhabited_time_str)
                    .lost_focus()
                {
                    if let Ok(mit) = self.max_inhabited_time_str.parse::<usize>() {
                       self.max_inhabited_time = mit;
                    } else {
                        self.errs.push("The maximum time a player can be in a chunk must be a positive integer".to_string());
                        println!("The maximum time a player can be in a chunk must be a positive integer");
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Number of processing threads");
                if ui.text_edit_singleline(&mut self.thread_count_str).lost_focus() {
                    if let Ok(tc) = self.thread_count_str.parse::<usize>() {
                        if tc >= num_cpus::get() {
                            self.errs.push("The number of processing threads must be less than the number of CPU cores".to_string());
                            println!("The number of processing threads must be less than the number of CPU cores");
                        } else {
                            self.thread_count = tc;
                        }
                     } else {
                        self.errs.push("The number of processing threads must be a positive integer".to_string());
                        println!("The number of processing threads must be a positive integer");
                    }
                }
            });

            if ui.button("World folder").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.world_path = Some(path.display().to_string());
                }
            }

            if ui.button("Backup folder").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.backup_path = Some(path.display().to_string());
                }
            }

            ui.checkbox(&mut self.accept, "I accept that this action can damage my world");
                

            if let Some(picked_path) = &self.world_path {
                ui.horizontal(|ui| {
                    ui.label("World folder: ");
                    ui.monospace(picked_path);
                });
            }

            if let Some(picked_path) = &self.backup_path {
                ui.horizontal(|ui| {
                    ui.label("Backup to: ");
                    ui.monospace(picked_path);
                });
            }

            if ui.button("Delete unused chunks").clicked() {
                let mut err = !self.errs.is_empty();

                if self.world_path.is_none() {
                    self.errs.push("Must specify a world folder".to_string());
                    println!("Must specify a world folder");
                    err = true;
                }

                if self.backup_path.is_none() {
                    self.errs.push("Must specify a backup folder".to_string());
                    println!("Must specify a backup folder");
                    err = true;
                }

                if !self.accept {
                    self.errs.push("Must accpet that this operation is potentially dammaging".to_string());
                    println!("Must accpet that this operation is potentially dammaging");
                    err = true;
                }

                if !err {
                    if let Err(e) = launch(
                        self.world_path.as_ref().unwrap().to_string(),
                        self.backup_path.as_ref().unwrap().to_string(), 
                     self.max_inhabited_time, 
                     self.thread_count
                    ) {
                        self.errs.push(e.to_string());
                        println!("{e}");
                    }
                    // err = true;
                }
            }
            
            if !self.errs.is_empty() {
                ui.label("ERRORS:");
                for e in &self.errs {
                    ui.label(e);
                }
                // self.errs.clear();
            }

            /* 
            if !self.errs.is_empty() {
                ui.label("ERRORS:");
            }
            */
        });
    }
}

fn launch(world_path: String, backup_path: String, max_inhabited_time: usize, thread_count: usize) -> Result<(), Error> {
    let r = fs_extra::dir::copy(&world_path, &backup_path, &CopyOptions::default());
    if r.is_err() {
        return Err(Error::WorldFolderNotFound);
    }
    
    println!("Copied");

    let config = Config {
        world_folder: PathBuf::from(world_path),
        max_inhabited_time: max_inhabited_time,
        thread_count: thread_count // .unwrap_or(num_cpus::get()),
    };

    println!("Compressing");

    lessanvil::execute(config)?;

    println!("Compressed");
    Ok(())
}
