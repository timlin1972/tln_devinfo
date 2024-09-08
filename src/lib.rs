use anstream::println;
use owo_colors::OwoColorize as _;
use readable::up::*;
use serde::{Deserialize, Serialize};
use tracing::{span, Level};

use common::{plugin, utils};

const MODULE: &str = "devinfo";
const HOUSE_KEEPINT_TIMEOUT: u64 = 300; // copied from center::main.rs
const SHOW: &str = r#"
action plugin devinfo update {json}
    Update devinfo database. (usually from mqtt)

action plugin devinfo refresh all
    To ask all devices in devinfo to send sysinfo.
"#;

#[derive(Serialize, Deserialize, Debug)]
struct Report {
    topic: String,
    payload: String,
}

fn get_color_temperature(temperature: f32) -> String {
    let temp_str = format!("{temperature}");
    match temperature {
        t if t <= 60.0 => temp_str.green().to_string(),
        t if t > 60.0 && t <= 85.0 => temp_str.yellow().to_string(),
        t if t > 85.0 => temp_str.red().to_string(),
        _ => temp_str,
    }
}

fn handle_onboard(tx: &crossbeam_channel::Sender<String>, name: &str, prev: bool, curr: bool) {
    fn get_color_onboard(name: &str, prev: bool, curr: bool) -> String {
        fn get_color_on_off(prev: bool, curr: bool) -> String {
            let log = format!("{prev} -> {curr}");
            if curr {
                log.green().to_string()
            } else {
                log.red().to_string()
            }
        }

        format!(
            "[{}] {name}: {}",
            MODULE.blue(),
            get_color_on_off(prev, curr),
        )
    }

    let log = get_color_onboard(name, prev, curr);
    println!("{log}");

    let log = utils::encrypt(&log);
    tx.send(format!("send plugin logs add '{log}'")).unwrap();
}

#[derive(Serialize, Deserialize, Debug)]
struct CmdUpdate {
    name: String,
    onboard: Option<bool>,
    uptime: Option<u64>,
    hostname: Option<String>,
    os: Option<String>,
    temperature: Option<f32>,
    sw_uptime: Option<u64>,
}

struct DevInfo {
    name: String,
    onboard: bool,
    uptime: u64,
    hostname: String,
    os: String,
    last_update: u64,
    temperature: f32,
    sw_uptime: u64,
}

pub struct Plugin {
    devinfo: Vec<DevInfo>,
    tx: crossbeam_channel::Sender<String>,
}

impl Plugin {
    pub fn new(tx: &crossbeam_channel::Sender<String>) -> Plugin {
        println!("[{}] Loading...", MODULE.blue());

        let _ = tracing_subscriber::fmt::try_init();

        let span = span!(Level::INFO, MODULE);
        let _enter = span.enter();

        Plugin {
            devinfo: vec![],
            tx: tx.clone(),
        }
    }
}

impl plugin::Plugin for Plugin {
    fn name(&self) -> &str {
        MODULE
    }

    fn show(&mut self) -> String {
        println!("[{}]", MODULE.blue());

        let mut show = String::new();
        show += SHOW;

        println!("{show}");

        show
    }

    fn action(&mut self, action: &str, data: &str, _data2: &str) -> String {
        match action {
            "update" => {
                let last_update = utils::get_ts();

                let cmd_update: CmdUpdate = serde_json::from_str(data).unwrap();

                // device existed
                if let Some(dev) = self
                    .devinfo
                    .iter_mut()
                    .find(|dev| dev.name == cmd_update.name)
                {
                    if let Some(t) = cmd_update.onboard {
                        if dev.onboard != t {
                            handle_onboard(&self.tx, &cmd_update.name, dev.onboard, t);
                        }
                        dev.onboard = t;
                    }
                    if let Some(t) = cmd_update.uptime {
                        dev.uptime = t;
                    }
                    if let Some(t) = cmd_update.hostname {
                        dev.hostname = t;
                    }
                    if let Some(t) = cmd_update.os {
                        dev.os = t;
                    }
                    if let Some(t) = cmd_update.temperature {
                        dev.temperature = t;
                    }
                    if let Some(t) = cmd_update.sw_uptime {
                        dev.sw_uptime = t;
                    }

                    dev.last_update = last_update;
                }
                // device NOT existed
                else {
                    if let Some(t) = cmd_update.onboard {
                        if t {
                            handle_onboard(&self.tx, &cmd_update.name, false, true);
                        }
                    }
                    self.devinfo.push(DevInfo {
                        name: cmd_update.name,
                        onboard: cmd_update.onboard.unwrap_or(true),
                        uptime: cmd_update.uptime.unwrap_or(0),
                        hostname: cmd_update.hostname.unwrap_or("n/a".to_owned()),
                        os: cmd_update.os.unwrap_or("n/a".to_owned()),
                        last_update,
                        temperature: cmd_update.temperature.unwrap_or(0.0),
                        sw_uptime: cmd_update.sw_uptime.unwrap_or(0),
                    });
                }
            }
            "refresh" => {
                if data == "all" {
                    self.devinfo.iter().for_each(|dev| {
                        let report = Report {
                            topic: format!("tln/{}/send", dev.name),
                            payload: utils::encrypt("send plugin sysinfo report myself"),
                        };
                        let json_string = serde_json::to_string(&report).unwrap();

                        self.tx
                            .send(format!("send plugin mqtt report '{json_string}'"))
                            .unwrap();
                    });
                }
            }
            _ => (),
        }

        "send".to_owned()
    }

    fn status(&mut self) -> String {
        println!("[{}]", MODULE.blue());

        let mut status = String::new();

        let uptime = utils::get_ts();

        for dev in &self.devinfo {
            status += &format!("{}\n", dev.name.blue());
            status += &format!(
                "\tOnboard: {} (Last updated: {} ago)\n",
                if dev.onboard {
                    "true".green().bold().to_string()
                } else {
                    "false".red().to_string()
                },
                if uptime - dev.last_update > HOUSE_KEEPINT_TIMEOUT {
                    Uptime::from(uptime - dev.last_update).red().to_string()
                } else {
                    Uptime::from(uptime - dev.last_update).green().to_string()
                }
            );
            status += &format!("\tSW uptime: {}\n", Uptime::from(dev.sw_uptime));
            status += &format!(
                "\tTemperature: {}°C\n",
                get_color_temperature(dev.temperature)
            );
            status += &format!("\tUptime: {}\n", Uptime::from(dev.uptime));
            status += &format!("\tHostname: {}\n", dev.hostname);
            status += &format!("\tOs: {}\n", dev.os);
        }

        println!("{status}");

        status
    }
}

#[no_mangle]
pub extern "C" fn create_plugin(
    tx: &crossbeam_channel::Sender<String>,
) -> *mut plugin::PluginWrapper {
    let plugin = Box::new(Plugin::new(tx));
    Box::into_raw(Box::new(plugin::PluginWrapper::new(plugin)))
}

/// # Safety
#[no_mangle]
pub unsafe extern "C" fn unload_plugin(wrapper: *mut plugin::PluginWrapper) {
    if !wrapper.is_null() {
        unsafe {
            let _ = Box::from_raw(wrapper);
        }
    }
}
