//! Process-local samples; OS queries run on one worker, never on the render thread.
use eframe::egui;
use std::sync::{
	Once, OnceLock,
	atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct Sample {
	ready: AtomicBool,
	unavailable: AtomicBool,
	memory: AtomicU64,
	cpu: AtomicU32,
	cpus: AtomicU32,
}

static SAMPLE: OnceLock<Sample> = OnceLock::new();
static START_WORKER: Once = Once::new();

pub fn show(ctx: &egui::Context) {
	// Publish storage before starting the worker: the first sample cannot race initialization.
	let sample = SAMPLE.get_or_init(Sample::default);
	START_WORKER.call_once(|| {
		if std::thread::Builder::new()
			.name("rustcord-resources".into())
			.spawn(move || {
				let Ok(pid) = sysinfo::get_current_pid() else {
					sample.unavailable.store(true, Ordering::Relaxed);
					return;
				};
				let mut system = sysinfo::System::new();
				let refresh = sysinfo::ProcessRefreshKind::nothing()
					.with_memory()
					.with_cpu()
					.without_tasks();
				// CPU usage needs two measurements separated by an interval, not a fake zero
				// from the first refresh. Only the current process is queried.
				system.refresh_processes_specifics(
					sysinfo::ProcessesToUpdate::Some(&[pid]),
					true,
					refresh,
				);
				system.refresh_cpu_usage();
				loop {
					std::thread::sleep(Duration::from_secs(1));
					system.refresh_processes_specifics(
						sysinfo::ProcessesToUpdate::Some(&[pid]),
						true,
						refresh,
					);
					let Some(process) = system.process(pid) else {
						sample.ready.store(false, Ordering::Release);
						sample.unavailable.store(true, Ordering::Relaxed);
						return;
					};
					sample.memory.store(process.memory(), Ordering::Relaxed);
					sample
						.cpu
						.store(process.cpu_usage().max(0.0).to_bits(), Ordering::Relaxed);
					sample.cpus.store(system.cpus().len().max(1) as u32, Ordering::Relaxed);
					sample.ready.store(true, Ordering::Release);


				}
			})
			.is_err()
		{
			sample.unavailable.store(true, Ordering::Relaxed);
		}
	});
	let label = if sample.unavailable.load(Ordering::Relaxed) {
		"Resource monitor unavailable".into()
	} else if sample.ready.load(Ordering::Acquire) {
		let num_cpus = sample.cpus.load(Ordering::Relaxed).max(1) as f32;
		let raw_cpu = f32::from_bits(sample.cpu.load(Ordering::Relaxed));
		let cpu_pct = (raw_cpu / num_cpus).round().clamp(0.0, 100.0);
		format!(
			"CPU {:.0}%  ·  RAM {:.0} MiB",
			cpu_pct,
			sample.memory.load(Ordering::Relaxed) as f64 / 1_048_576.0
		)
	} else {
		"CPU —%  ·  RAM —".into()
	};
	egui::Area::new(egui::Id::unique("rustcord-resource-monitor"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -6.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
                ui.label(egui::RichText::new(label).size(10.0))
                    .on_hover_text("Rustcord main process · refreshed once per second\nRAM is resident memory (RSS). CPU 100% means one logical core.\nTemporary sign-in and media helper processes are excluded.");
            });
        });
	ctx.request_repaint_after(Duration::from_secs(1));
}
