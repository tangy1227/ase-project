use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, StreamConfig};

use anyhow::{bail, Context, Error};
use egui::{vec2, Vec2, Widget, Key};
use egui_extras;
use hound::{WavReader, WavWriter, SampleFormat};


use proj::reader::{Filter, OpenOptions, Sofar};
use proj::render::Renderer;

use ringbuf::HeapRb;

use core::slice;
use std::borrow::Borrow;
use std::cmp::min;
use std::sync::{Arc, Condvar, Mutex};
use std::{env, io::Read};
use std::{thread, time};
use eframe::egui;

// Rotation in radians to apply to object position every 50 ms
const ROTATION: f32 = 2.0 / 180.0 * std::f32::consts::PI;
// Single block size in frames
const BLOCK_LEN: usize = 1024;


fn main()
{
    println!("Rust EGUI Running!!!");
    

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([820.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "3D Rendering for Monophonic Audio",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::<MyApp>::default()
            
        })
    ).unwrap();
    return;
}

struct MyApp {
    x_pos : f32,
    x_pos_str : String,
    y_pos : f32,
    y_pos_str : String, 
    z_pos : f32,
    z_pos_str : String,
    input_wav_file_name : String,
    input_sofa_file_name : String,
    output_wav_file_name : String,
    left_source_x_pos : f32,
    left_source_y_pos : f32, 
    right_source_x_pos : f32,
    right_source_y_pos : f32
}
impl Default for MyApp {
    fn default() -> Self {
        Self {
            x_pos : 0.0,
            x_pos_str : "0.0".to_string(),
            y_pos : 0.0,
            y_pos_str : "0.0".to_string(),
            z_pos : 0.0,
            z_pos_str : "0.0".to_string(),
            input_wav_file_name : "No input selected".to_string(),
            input_sofa_file_name : "audio/default.sofa".to_string(),
            output_wav_file_name : "Please select input first".to_string(),
            left_source_x_pos : -10.0,
            left_source_y_pos : -10.0, 
            right_source_x_pos : -10.0,
            right_source_y_pos : -10.0
        }
    }
}
impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) 
    {
        egui::CentralPanel::default().show(ctx, |ui| 
            {
            
                if ctx.input(|k| k.key_pressed(Key::Space))
                {
                    let pointer_pos = ctx.pointer_hover_pos().unwrap();
                    println!("{}, {}", pointer_pos.x, pointer_pos.y);
                }
                //ui.heading("3D Rendering for Monophonic Audio");
                egui_extras::install_image_loaders(ctx);
                let floating_image_left = egui::Image::new(egui::include_image!("../images/dot.png"));
                //= ui.image(egui::include_image!("../images/pos1.png")).;
                let floating_image_right = egui::Image::new(egui::include_image!("../images/dot.png"));
                ui.horizontal(|ui| 
                {
                    let x_pos_label = ui.label("X-position (-10 to 10): ");
                    let text_edit = egui::TextEdit::singleline(&mut self.x_pos_str)
                                        .desired_width(100.00);
                    ui.add(text_edit)
                        .labelled_by(x_pos_label.id);
                    if ui.button("Set X position").clicked()
                    {
                        self.x_pos = self.x_pos_str.parse::<f32>().unwrap();
                        println!("x pos changed to {}", self.x_pos);
                        let left_pos = get_left_pos(self.y_pos, self.z_pos);
                        self.left_source_x_pos = left_pos.x;
                        self.left_source_y_pos = left_pos.y; 
                        let right_pos = get_right_pos(self.x_pos, self.z_pos);
                        self.right_source_x_pos = right_pos.x;
                        self.right_source_y_pos = right_pos.y;
                    }
                    let mut x_true_label = ui.label("Actual X position: ".to_string());
                    let mut x_true_label_number = ui.label(self.x_pos.to_string());

                });
                ui.horizontal(|ui|
                {
                    let y_pos_label = ui.label("Y-position (-10 to 10): ");
                    let text_edit = egui::TextEdit::singleline(&mut self.y_pos_str)
                                        .desired_width(100.00);
                    ui.add(text_edit)
                        .labelled_by(y_pos_label.id);
                    if ui.button("Set Y position").clicked()
                    {
                        self.y_pos = self.y_pos_str.parse::<f32>().unwrap();
                        println!("y pos changed to {}", self.y_pos);
                        let left_pos = get_left_pos(self.y_pos, self.z_pos);
                        self.left_source_x_pos = left_pos.x;
                        self.left_source_y_pos = left_pos.y; 
                        let right_pos = get_right_pos(self.x_pos, self.z_pos);
                        self.right_source_x_pos = right_pos.x;
                        self.right_source_y_pos = right_pos.y;
                    }
                    let mut y_true_label = ui.label("Actual Y position: ".to_string());
                    let mut y_true_label_number = ui.label(self.y_pos.to_string());
                });
                ui.horizontal(|ui|
                {
                    let z_pos_label = ui.label("Z-position (-10 to 10): ");
                    let text_edit = egui::TextEdit::singleline(&mut self.z_pos_str)
                                        .desired_width(100.00);
                    ui.add(text_edit)
                        .labelled_by(z_pos_label.id);
                    if ui.button("Set Z position").clicked()
                    {
                        self.z_pos = self.z_pos_str.parse::<f32>().unwrap();
                        println!("z pos changed to {}", self.z_pos);
                        let left_pos = get_left_pos(self.y_pos, self.z_pos);
                        self.left_source_x_pos = left_pos.x;
                        self.left_source_y_pos = left_pos.y; 
                        let right_pos = get_right_pos(self.x_pos, self.z_pos);
                        self.right_source_x_pos = right_pos.x;
                        self.right_source_y_pos = right_pos.y;
                    }
                    let mut z_true_label = ui.label("Actual Z position: ".to_string());
                    let mut z_true_label_number = ui.label(self.z_pos.to_string());

                });
                ui.horizontal(|ui|
                {
                    let mut update_button = ui.button("Update all positions");

                    if update_button.clicked()
                    {
                        self.x_pos = self.x_pos_str.parse::<f32>().unwrap();
                        self.y_pos = self.y_pos_str.parse::<f32>().unwrap();
                        self.z_pos = self.z_pos_str.parse::<f32>().unwrap();

                        let left_pos = get_left_pos(self.y_pos, self.z_pos);
                        self.left_source_x_pos = left_pos.x;
                        self.left_source_y_pos = left_pos.y; 
                        let right_pos = get_right_pos(self.x_pos, self.z_pos);
                        self.right_source_x_pos = right_pos.x;
                        self.right_source_y_pos = right_pos.y;
                    }
                });

                ui.horizontal(|ui|
                {
                    ui.vertical(|ui|
                    {
                        ui.set_max_size(Vec2::new(400.0, 318.0));
                        ui.image(egui::include_image!("../images/pos1.png"));
                    });
                    ui.vertical(|ui|
                    {
                        ui.set_max_size(Vec2::new(400.0, 318.0));
                        ui.image(egui::include_image!("../images/pos2.png"));
                    });
                    
                });
                ui.horizontal(|ui|
                {
                    if ui.button("Select input WAV file (...)").clicked()
                    {
                        let ret = rfd::FileDialog::new().pick_file().unwrap();
                        let ret = ret.to_str().unwrap();
                        self.input_wav_file_name = ret.to_string();

                        let input_filename_length = self.input_wav_file_name.len();
                        let input_filename_path = &self.input_wav_file_name[0..input_filename_length - 4];
                        let mut output_filename = "".to_string();
                        output_filename.push_str(input_filename_path);
                        output_filename.push_str("_converted_");
                        output_filename.push_str(&self.x_pos_str);
                        output_filename.push_str("_");
                        output_filename.push_str(&self.y_pos_str);
                        output_filename.push_str("_");
                        output_filename.push_str(&self.z_pos_str);
                        output_filename.push_str("_");
                        output_filename.push_str(".wav");
                        self.output_wav_file_name = output_filename;

                    }
                    let mut input_wav_file_label = egui::Label::new(self.input_wav_file_name.to_owned());
                    ui.add(input_wav_file_label);
                });
                ui.horizontal(|ui|
                {
                    if ui.button("Select input SOFA file (...)").clicked()
                    {
                        let ret = rfd::FileDialog::new().pick_file().unwrap();
                        let ret = ret.to_str().unwrap();
                        self.input_sofa_file_name = ret.to_string();
                    }
                    let mut input_sofa_file_label = egui::Label::new(self.input_sofa_file_name.to_owned());
                    ui.add(input_sofa_file_label);
                });

                ui.horizontal(|ui|
                    {
                        let mut output_wav_file_title_label = egui::Label::new("The output WAV file is:");
                        ui.add(output_wav_file_title_label);
                        let mut output_wav_file_label = egui::Label::new(self.output_wav_file_name.to_owned());
                        ui.add(output_wav_file_label);
                    });
                ui.horizontal(|ui|
                    {
                        ui.vertical(|ui|
                            {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui|
                                {
                                    ui.set_min_size(egui::vec2(360.0, 80.0));
                                    let mut conversion_button = egui::Button::new("Start conversion!")
                                                    .min_size(egui::vec2(360.0, 80.0));
                                    let add_result = ui.add(conversion_button);

                                    if add_result.clicked()
                                    {
                                        println!("Start converting...");
                                        //test_conversion();
                                         
                                        convert_audio(self.x_pos, 
                                            self.y_pos, 
                                            self.z_pos,
                                            self.input_wav_file_name.clone(),
                                            self.input_sofa_file_name.clone(),
                                        self.output_wav_file_name.clone());
                                        
                                        
                                    }
                                });
                            });
                        ui.vertical(|ui|
                            {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui|
                                {
                                    /*
                                    ui.set_min_size(egui::vec2(360.0, 80.0));
                                    let mut play_button = egui::Button::new("Play converted audio")
                                                    .min_size(egui::vec2(360.0, 80.0));
                                    let add_result = ui.add(play_button);

                                    if add_result.clicked()
                                    {
                                        println!("Start playing...");
                                        self.left_source_x_pos += 100.0;
                                        self.left_source_y_pos += 100.0;
                                    }
                                    */
                                });
                                
                            });

                    });
                let left_rect = egui::Rect::from_center_size(egui::Pos2::new(self.left_source_x_pos, self.left_source_y_pos), 
                egui::Vec2::new(10.0, 10.0));
                let right_rect = egui::Rect::from_center_size(egui::Pos2::new(self.right_source_x_pos, self.right_source_y_pos),
                egui::Vec2::new(10.0, 10.0));
                floating_image_left.paint_at(ui, left_rect);
                floating_image_right.paint_at(&ui, right_rect);
                //ui.put(left_rect, floating_image_left);
            });
            

    
    }
}

fn convert_audio(x_pos : f32, y_pos : f32, z_pos : f32, wav_file : String, sofa_file : String,
                    output_file : String) 
{
    let wav_file = "audio/test.wav".to_string();
    let sofa_file = "audio/test_sofa_1.sofa".to_string();
    println!("{}", sofa_file);
    println!("{}", wav_file);
    let mut reader = WavReader::open(wav_file).unwrap();
    let sample_rate = reader.spec().sample_rate;
    let sofa = OpenOptions::new()
                    .sample_rate(reader.spec().sample_rate as f32)
                    .open(sofa_file)
                    .unwrap();
    // define the sofa filter
    let filter_length = sofa.filter_len();
    println!("filter length = {filter_length}");
    let mut filter = Filter::new(filter_length);
    sofa.filter(x_pos, y_pos, z_pos, &mut filter);
    // define the render
    let mut render = Renderer::builder(filter_length)
                .with_sample_rate(sample_rate as f32)
                .with_partition_len(64)
                .build()
                .unwrap();
    render.set_filter(&filter).unwrap();
    
    // get the data?
    let total_block = reader.len() / BLOCK_LEN as u32;
    let mut final_output : Vec<Vec<f32>> = vec![vec![], vec![]];
    for cur_block in 0..total_block
    {
        let src = reader.samples::<f32>()
                                .take(BLOCK_LEN)
                                .collect::<Result<Vec<_>, _>>() 
                                .unwrap();
        // get the results
        let mut left = vec![1.0; BLOCK_LEN];
        let mut right = vec![1.0; BLOCK_LEN];
        
        render.process_block(src, &mut left, &mut right).unwrap();

        for (l, r) in Iterator::zip(left.iter(), right.iter()) {
            //println!("{}, {}", *l, *r);
            final_output[0].push(*l);
            final_output[1].push(*r);
        }
        
    }
    println!("{}", final_output[0].len());
    write_converted_file(final_output, output_file, sample_rate);
    println!("Done");
    
}

fn write_converted_file(converted_data : Vec<Vec<f32>>, output_file : String, sample_rate : u32)
{
    let write_spec = hound::WavSpec {
        channels: 2 as u16,
        sample_rate : sample_rate, 
        bits_per_sample: 32, 
        sample_format: SampleFormat::Float, // Integer samples
    };

    let mut writer = hound::WavWriter::create(output_file, write_spec).unwrap();
    let total_length = min(converted_data[0].len(), converted_data[1].len());
    for data_index in 0..total_length
    {
        for channel in 0..2
        {
            let _ = writer.write_sample(converted_data[channel][data_index]);
        }
    }
    writer.finalize().unwrap();

}

fn get_left_pos(y_pos : f32, z_pos : f32) -> egui::Pos2
{
    let y_center = 31.0 + 180.0;
    let z_center = 110.0 + 142.5;
    let y_pos = y_center + (y_pos/10.0)*(360.0/2.0);
    let z_pos = z_center - (z_pos/10.0)*(285.0/2.0);
    let left_pos = egui::Pos2::new(y_pos, z_pos);
    left_pos 
}

fn get_right_pos(x_pos : f32, z_pos : f32) -> egui::Pos2
{
    let x_center = 438.0 + 180.0;
    let z_center = 110.0 + 142.5;
    let x_pos = x_center + (x_pos/10.0)*(360.0/2.0);
    let z_pos = z_center - (z_pos/10.0)*(285.0/2.0);
    let left_pos = egui::Pos2::new(x_pos, z_pos);
    left_pos 
}