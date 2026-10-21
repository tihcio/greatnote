//! Memoria del computer e schede video, per indicare quali modelli sono
//! adatti (ADR 0021). Le schede video sono quelle che vede il motore
//! (`llama-server --list-devices`), quindi proprio quelle che userà.
//!
//! Le stime sono indicative: servono a scegliere, non a garantire.

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

const MIB: u64 = 1024 * 1024;
const LIST_DEVICES_TIMEOUT: Duration = Duration::from_secs(20);
/// Memoria oltre ai pesi: contesto, buffer di calcolo e il processo stesso.
const RUNTIME_OVERHEAD: u64 = 768 * MIB;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GpuDevice {
    pub name: String,
    pub memory_bytes: u64,
    /// Memoria video propria. Le schede integrate usano la RAM del sistema
    /// e non vanno sommate a essa.
    pub dedicated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HardwareInfo {
    /// RAM totale, se il sistema la indica.
    pub ram_bytes: Option<u64>,
    pub gpus: Vec<GpuDevice>,
}

impl HardwareInfo {
    /// Memoria video dedicata della scheda più capiente.
    pub fn dedicated_vram(&self) -> u64 {
        self.gpus
            .iter()
            .filter(|gpu| gpu.dedicated)
            .map(|gpu| gpu.memory_bytes)
            .max()
            .unwrap_or(0)
    }
}

/// Quanto un modello è adatto a questo computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelFit {
    /// Sta tutto nella memoria della scheda video: risposte rapide.
    Gpu,
    /// Sta nella RAM (in parte anche sulla scheda video): più lento.
    Cpu,
    /// Occupa quasi tutta la memoria: può rallentare il computer.
    Tight,
    /// Non ci sta.
    TooLarge,
    /// Memoria del computer sconosciuta.
    Unknown,
}

/// Memoria stimata per eseguire un modello di `size_bytes` (pesi più
/// contesto e buffer).
pub fn estimated_memory(size_bytes: u64) -> u64 {
    size_bytes
        .saturating_add(size_bytes / 5)
        .saturating_add(RUNTIME_OVERHEAD)
}

pub fn model_fit(size_bytes: u64, hardware: &HardwareInfo) -> ModelFit {
    let needed = estimated_memory(size_bytes);
    let vram = hardware.dedicated_vram();
    if vram >= needed {
        return ModelFit::Gpu;
    }
    let Some(ram) = hardware.ram_bytes else {
        return ModelFit::Unknown;
    };
    // Una parte della RAM resta al sistema e alle altre applicazioni; gli
    // strati che non stanno sulla scheda video restano in RAM.
    if (ram / 10 * 6).saturating_add(vram) >= needed {
        ModelFit::Cpu
    } else if (ram / 100 * 85).saturating_add(vram) >= needed {
        ModelFit::Tight
    } else {
        ModelFit::TooLarge
    }
}

/// RAM e schede video. `engine` è `llama-server`, se disponibile.
pub fn detect(engine: Option<&Path>) -> HardwareInfo {
    HardwareInfo {
        ram_bytes: total_ram(),
        gpus: engine.map(list_devices).unwrap_or_default(),
    }
}

/// Schede video viste dal motore, con un tempo massimo: un driver bloccato
/// non deve bloccare le impostazioni.
fn list_devices(engine: &Path) -> Vec<GpuDevice> {
    let mut command = Command::new(engine);
    command
        .arg("--list-devices")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = engine.parent() {
        command.current_dir(directory);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            tracing::warn!("elenco delle schede video non disponibile: {error}");
            return Vec::new();
        }
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < LIST_DEVICES_TIMEOUT => {
                thread::sleep(Duration::from_millis(100));
            }
            _ => {
                tracing::warn!("elenco delle schede video interrotto: tempo scaduto");
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
        }
    }
    // L'elenco è breve: il processo è già terminato senza riempire i canali.
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut output);
    }
    parse_devices(&output)
}

/// Righe come `Vulkan1: NVIDIA GeForce RTX 3070 Laptop GPU (8018 MiB, 7250
/// MiB free)` dopo `Available devices:`.
pub fn parse_devices(output: &str) -> Vec<GpuDevice> {
    output
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("Available devices"))
        .skip(1)
        .filter_map(parse_device_line)
        .collect()
}

fn parse_device_line(line: &str) -> Option<GpuDevice> {
    let (_, description) = line.trim().split_once(": ")?;
    let open = description.rfind(" (")?;
    let name = description[..open].trim();
    let memory = description[open + 2..].strip_suffix(')')?;
    let (total, free) = memory.split_once(", ")?;
    let total: u64 = total.strip_suffix(" MiB")?.trim().parse().ok()?;
    let free: u64 = free.strip_suffix(" MiB free")?.trim().parse().ok()?;
    let software = ["llvmpipe", "lavapipe", "SwiftShader", "Microsoft Basic"]
        .iter()
        .any(|marker| name.contains(marker));
    if name.is_empty() || total == 0 || software {
        return None;
    }
    // Le schede integrate dichiarano come propria una parte della RAM (e a
    // volte più memoria libera che totale): non è memoria video dedicata.
    let integrated_name = [
        "Iris",
        "UHD Graphics",
        "HD Graphics",
        "Radeon(TM) Graphics",
        "Radeon Graphics",
        "Radeon Vega",
        "Adreno",
        "Apple M",
    ]
    .iter()
    .any(|marker| name.contains(marker));
    Some(GpuDevice {
        name: name.chars().filter(|c| !c.is_control()).take(120).collect(),
        memory_bytes: total.saturating_mul(MIB),
        dedicated: !integrated_name && free <= total,
    })
}

#[cfg(windows)]
fn total_ram() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    // SAFETY: MEMORYSTATUSEX contiene solo interi, quindi la struttura
    // azzerata è valida; `dwLength` è impostato come richiesto dall'API e il
    // puntatore resta valido per tutta la chiamata.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
}

#[cfg(target_os = "linux")]
fn total_ram() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo(&meminfo)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn total_ram() -> Option<u64> {
    None
}

#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn parse_meminfo(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib: u64 = line
        .trim_start_matches("MemTotal:")
        .trim()
        .strip_suffix("kB")?
        .trim()
        .parse()
        .ok()?;
    Some(kib.saturating_mul(1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * MIB;

    #[test]
    fn devices_are_parsed_and_integrated_graphics_is_not_dedicated() {
        // Uscita reale di llama-server b11397 su un portatile con due schede.
        let output = "ggml_vulkan: Found 2 Vulkan devices:\n\
            Available devices:\n  \
            Vulkan0: Intel(R) Iris(R) Xe Graphics (24427 MiB, 31703 MiB free)\n  \
            Vulkan1: NVIDIA GeForce RTX 3070 Laptop GPU (8018 MiB, 7250 MiB free)\n";
        let devices = parse_devices(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Intel(R) Iris(R) Xe Graphics");
        assert!(!devices[0].dedicated);
        assert_eq!(devices[1].name, "NVIDIA GeForce RTX 3070 Laptop GPU");
        assert_eq!(devices[1].memory_bytes, 8018 * MIB);
        assert!(devices[1].dedicated);

        let software = "Available devices:\n  Vulkan0: llvmpipe (LLVM 17.0.6, 256 bits) (7900 MiB, 7900 MiB free)\n";
        assert!(parse_devices(software).is_empty());
        assert!(parse_devices("Available devices:\n").is_empty());
        assert!(parse_devices("errore qualsiasi").is_empty());
        assert!(parse_devices("Available devices:\n  Vulkan0: Scheda (tanta, poca)\n").is_empty());
    }

    #[test]
    fn fit_prefers_dedicated_video_memory_then_ram() {
        let laptop = HardwareInfo {
            ram_bytes: Some(16 * GIB),
            gpus: vec![
                GpuDevice {
                    name: "Intel(R) Iris(R) Xe Graphics".to_owned(),
                    memory_bytes: 8 * GIB,
                    dedicated: false,
                },
                GpuDevice {
                    name: "NVIDIA".to_owned(),
                    memory_bytes: 8 * GIB,
                    dedicated: true,
                },
            ],
        };
        assert_eq!(laptop.dedicated_vram(), 8 * GIB);
        assert_eq!(model_fit(3 * GIB, &laptop), ModelFit::Gpu);
        // 8 GB di pesi: non sta sulla scheda video, sta in RAM con la scheda.
        assert_eq!(model_fit(8 * GIB, &laptop), ModelFit::Cpu);
        assert_eq!(model_fit(15 * GIB, &laptop), ModelFit::Tight);
        assert_eq!(model_fit(40 * GIB, &laptop), ModelFit::TooLarge);

        let office = HardwareInfo {
            ram_bytes: Some(8 * GIB),
            gpus: Vec::new(),
        };
        assert_eq!(model_fit(GIB, &office), ModelFit::Cpu);
        assert_eq!(model_fit(5 * GIB, &office), ModelFit::Tight);
        assert_eq!(model_fit(7 * GIB, &office), ModelFit::TooLarge);
        assert_eq!(model_fit(GIB, &HardwareInfo::default()), ModelFit::Unknown);
    }

    #[test]
    fn meminfo_total_is_read_in_bytes() {
        let meminfo = "MemTotal:       16308412 kB\nMemFree:         1234 kB\n";
        assert_eq!(parse_meminfo(meminfo), Some(16_308_412 * 1024));
        assert_eq!(parse_meminfo("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn total_ram_is_available_on_supported_systems() {
        if cfg!(any(windows, target_os = "linux")) {
            assert!(total_ram().is_some_and(|ram| ram > 256 * MIB));
        }
    }
}
