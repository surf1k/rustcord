//! Owner-started quest progress jobs. No background work before a deliberate click.
use client_core::auth::SessionSecret;
use discord_api::{
	DiscordApi,
	proxy::ApiProxy,
	quests::{MAX_RUN_SECONDS, Quest, Task, VoiceContext},
};
use eframe::egui;
use model::Id;
use std::{
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::{
	runtime::Runtime,
	sync::{mpsc, watch},
	task::JoinHandle,
};

pub(super) struct Session {
	pub secret: Option<Arc<SessionSecret>>,
	pub generation: u64,
	pub offline: bool,
	pub proxy: watch::Receiver<Option<ApiProxy>>,
	pub voice: Option<VoiceContext>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
	generation: u64,
	account: usize,
}
impl Session {
	fn key(&self) -> Key {
		Key {
			generation: self.generation,
			account: self.secret.as_ref().map_or(0, |s| Arc::as_ptr(s) as usize),
		}
	}
}
struct Update {
	quests: Option<Vec<Quest>>,
	note: String,
	done: bool,
}
struct Job {
	task: JoinHandle<()>,
	updates: mpsc::Receiver<Update>,
	voice: Option<VoiceContext>,
}
#[derive(Clone, Copy)]
enum Action {
	Refresh,
	Run(Id),
	RunAll,
	Claim(Id),
}

pub(super) struct Quests {
	pub open: bool,
	key: Option<Key>,
	quests: Vec<Quest>,
	job: Option<Job>,
	note: String,
	auto_enroll: bool,
	auto_claim: bool,
}
impl Default for Quests {
	fn default() -> Self {
		Self {
			open: false,
			key: None,
			quests: Vec::new(),
			job: None,
			note: "Click Refresh or Auto Complete All to start.".into(),
			auto_enroll: true,
			auto_claim: true,
		}
	}
}
impl Drop for Quests {
	fn drop(&mut self) {
		if let Some(job) = self.job.take() {
			job.task.abort();
		}
	}
}
impl Quests {
	fn stop(&mut self, note: &str) {
		if let Some(job) = self.job.take() {
			job.task.abort();
		}
		self.note = note.into();
	}
	fn observe_session(&mut self, session: &Session) {
		let key = session.key();
		if self.key != Some(key) || session.offline || session.secret.is_none() {
			self.stop("Session changed. Refresh after signing in; any in-flight write may already have reached Discord.");
			self.quests.clear();
			self.key = Some(key);
		} else if self
			.job
			.as_ref()
			.is_some_and(|job| job.voice.is_some() && job.voice != session.voice)
		{
			self.stop(
				"Voice channel changed. Automation stopped; refresh to verify any in-flight result.",
			);
		}
	}
	pub fn show(&mut self, ctx: &egui::Context, runtime: &Runtime, session: Session) {
		self.observe_session(&session);
		let mut done = false;
		if let Some(job) = &mut self.job {
			while let Ok(update) = job.updates.try_recv() {
				if let Some(quests) = update.quests {
					self.quests = quests;
				}
				self.note = update.note;
				done |= update.done;
			}
			if job.task.is_finished() && job.updates.is_empty() {
				done = true;
			}
		}
		if done {
			self.job = None;
		}
		if self.job.is_some() {
			ctx.request_repaint_after(Duration::from_millis(100));
		}
		egui::Area::new(egui::Id::unique("rustcord-quests-button"))
			.anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -36.0])
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				if ui.button("Quests").clicked() {
					self.open = !self.open;
				}
			});
		if !self.open {
			return;
		}
		let mut open = self.open;
		let mut action = None;
		let mut stop = false;
		egui::Window::new("Discord Quests").id(egui::Id::unique("rustcord-quests"))
            .open(&mut open).default_width(620.0).default_height(480.0).resizable(true)
            .show(ctx,|ui| {
                ui.label("Unofficial quest progress controls. No game/process spoofing, challenge bypass or automatic OAuth grants.");
                ui.label("Video jobs report elapsed time; activity jobs require your current voice channel. Only Discord-confirmed completion counts.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.auto_enroll,"Auto enroll when Start is clicked");
                    ui.checkbox(&mut self.auto_claim,"Auto claim confirmed rewards");
                });
                ui.horizontal(|ui| {
                    let available=!session.offline && session.secret.is_some() && self.job.is_none();
                    if ui.add_enabled(available,egui::Button::new("Refresh")).clicked() {action=Some(Action::Refresh);}
                    if ui.add_enabled(available,egui::Button::new("⚡ Auto Complete All")).clicked() {action=Some(Action::RunAll);}
                    if ui.add_enabled(self.job.is_some(),egui::Button::new("Stop")).clicked() {stop=true;}
                    if session.offline || session.secret.is_none() {ui.label("Sign in to load quests.");}
                });
                ui.label(&self.note);ui.separator();
                egui::ScrollArea::vertical().max_height(600.0).show(ui,|ui| {
                    for quest in &self.quests {
                        ui.group(|ui| {
                            ui.set_min_width(ui.available_width().min(400.0));
                            ui.strong(&quest.name);
                            ui.label(format!("{} · {}",quest.task.label(),quest.key));
                            ui.label(format!("Server progress: {:.0} / {}",quest.status.value,quest.target));
                            let label=if quest.status.claimed {"Claimed"} else if quest.status.completed {"Completed by Discord"} else if quest.status.enrolled {"Enrolled"} else {"Not enrolled"};
                            ui.label(label);
                            if !quest.expires.is_empty() {ui.small(format!("Expires: {}",quest.expires));}
                            let active=self.job.is_some();
                            if quest.task==Task::Unsupported {
                                ui.small("Complete this task in the official client/game. Rustcord will not invent progress or authorize apps.");
                            } else if quest.task.needs_voice() && session.voice.is_none() {
                                ui.small("Join a voice channel before starting this activity task.");
                            }
                            ui.horizontal(|ui| {
                                let allowed=!active && quest.task!=Task::Unsupported && !quest.status.completed && (!quest.task.needs_voice() || session.voice.is_some());
                                if ui.add_enabled(allowed,egui::Button::new("Start")).clicked() {action=Some(Action::Run(quest.id));}
                                if ui.add_enabled(!active && quest.status.completed && !quest.status.claimed,egui::Button::new("Claim reward")).clicked() {action=Some(Action::Claim(quest.id));}
                            });
                        });
                    }
                });
            });
		self.open = open;
		if !open && self.job.is_some() {
			self.stop(
				"Quests window closed. Future requests stopped; refresh to verify any in-flight result.",
			);
		}
		if stop {
			self.stop(
				"Stopped. An in-flight write may already have reached Discord; refresh to verify.",
			);
		} else if let Some(action) = action {
			self.start(ctx, runtime, session, action);
		}
	}
	fn start(&mut self, ctx: &egui::Context, runtime: &Runtime, session: Session, action: Action) {
		if self.job.is_some() || session.offline {
			return;
		}
		let Some(secret) = session.secret else {
			return;
		};
		let api = match DiscordApi::with_proxy(secret, session.proxy) {
			Ok(api) => api,
			Err(error) => {
				self.note = error.label().into();
				return;
			}
		};
		let (send, updates) = mpsc::channel(1);
		let wake = ctx.clone();
		let enroll = self.auto_enroll;
		let claim = self.auto_claim;
		let voice = session.voice;
		self.note = "Loading server state…".into();
		let task = runtime.spawn(async move {
			let result = tokio::time::timeout(
				Duration::from_secs(u64::from(MAX_RUN_SECONDS)),
				run(&api, action, voice, enroll, claim, &send, &wake),
			)
			.await;
			let note = match result {
				Ok(Ok(note)) => note,
				Ok(Err(note)) => note,
				Err(_) => {
					"Stopped after 25 minutes. Completion is unconfirmed; refresh before retrying."
						.into()
				}
			};
			let _ = send
				.send(Update {
					quests: None,
					note,
					done: true,
				})
				.await;
			wake.request_repaint();
		});
		self.job = Some(Job {
			task,
			updates,
			voice: match action {
				Action::Run(_) => voice,
				_ => None,
			},
		});
	}
}
fn find(list: &[Quest], id: Id) -> Result<Quest, String> {
	list.iter()
		.find(|q| q.id == id)
		.cloned()
		.ok_or_else(|| "The quest is no longer available. Refresh the list.".into())
}
async fn send_update(
	send: &mpsc::Sender<Update>,
	ctx: &egui::Context,
	list: &[Quest],
	note: String,
) -> Result<(), String> {
	send.send(Update {
		quests: Some(list.to_vec()),
		note,
		done: false,
	})
	.await
	.map_err(|_| "Quest window closed.".to_owned())?;
	ctx.request_repaint();
	Ok(())
}
#[derive(Clone, Copy)]
struct QuestFlags {
	auto_enroll: bool,
	auto_claim: bool,
}

async fn run(
	api: &DiscordApi,
	action: Action,
	voice: Option<VoiceContext>,
	auto_enroll: bool,
	auto_claim: bool,
	send: &mpsc::Sender<Update>,
	ctx: &egui::Context,
) -> Result<String, String> {
	let mut list = api.quests().await.map_err(|e| e.label().to_owned())?;
	send_update(send, ctx, &list, "Server state loaded.".into()).await?;
	let flags = QuestFlags { auto_enroll, auto_claim };
	match action {
		Action::Refresh => Ok("Server state loaded. Select a quest and click Start, or click Auto Complete All.".into()),
		Action::Run(id) => {
			run_single_quest(api, id, voice, flags, send, ctx, &mut list).await
		}
		Action::RunAll => {
			let uncompleted: Vec<Id> = list
				.iter()
				.filter(|q| !q.status.completed && q.task != Task::Unsupported && (!q.task.needs_voice() || voice.is_some()))
				.map(|q| q.id)
				.collect();
			if uncompleted.is_empty() {
				return Ok("All eligible quests are already completed or claimed.".into());
			}
			let total = uncompleted.len();
			let all_flags = QuestFlags { auto_enroll: true, auto_claim: true };
			for (idx, qid) in uncompleted.into_iter().enumerate() {
				let _ = send_update(
					send,
					ctx,
					&list,
					format!("Running quest {} of {}...", idx + 1, total),
				)
				.await;
				if let Err(err) = run_single_quest(api, qid, voice, all_flags, send, ctx, &mut list).await {
					let _ = send_update(
						send,
						ctx,
						&list,
						format!("Quest failed: {err}; continuing to next..."),
					)
					.await;
				}
			}
			Ok("Auto Complete All finished! State reloaded.".into())
		}
		Action::Claim(id) => {
			let quest = find(&list, id)?;
			api.claim_quest(&quest)
				.await
				.map_err(|e| e.label().to_owned())?;
			list = api.quests().await.map_err(|e| e.label().to_owned())?;
			let claimed = find(&list, id)?.status.claimed;
			send_update(
				send,
				ctx,
				&list,
				"Claim request finished; state reloaded.".into(),
			)
			.await?;
			Ok(if claimed {
				"Reward claim confirmed by Discord."
			} else {
				"Claim was sent, but Discord has not confirmed it. Inspect the official client before retrying."
			}
			.into())
		}
	}
}
async fn run_single_quest(
	api: &DiscordApi,
	id: Id,
	voice: Option<VoiceContext>,
	flags: QuestFlags,
	send: &mpsc::Sender<Update>,
	ctx: &egui::Context,
	list: &mut Vec<Quest>,
) -> Result<String, String> {
	let mut quest = find(list, id)?;
	if quest.task == Task::Unsupported {
		return Err(
			"This task requires the official client or game; automation was not started.".into(),
		);
	}
	if quest.task.needs_voice() && voice.is_none() {
		return Err("Join a voice channel before starting this activity task.".into());
	}
	if !quest.status.enrolled && !quest.status.completed {
		if !flags.auto_enroll {
			return Err(
				"Enroll in the official client or enable Auto enroll, then click Start.".into(),
			);
		}
		api.enroll_quest(&quest)
			.await
			.map_err(|e| e.label().to_owned())?;
		*list = api.quests().await.map_err(|e| e.label().to_owned())?;
		quest = find(list, id)?;
		if !quest.status.enrolled {
			return Err(
				"Enrollment was sent but not confirmed by Discord; refresh before retrying.".into(),
			);
		}
	}
	let start = Instant::now();
	let initial = quest.status.value;
	let mut stalled = 0;
	while !quest.status.completed {
		tokio::time::sleep(Duration::from_secs(if quest.task == Task::Video {
			8
		} else {
			20
		}))
		.await;
		let before = quest.status.value;
		let mut status = if quest.task == Task::Video {
			let timestamp = video_timestamp(initial, start.elapsed(), quest.target);
			api.quest_video_progress(&quest, timestamp).await
		} else {
			api.quest_heartbeat(&quest, voice, false).await
		}
		.map_err(|e| e.label().to_owned())?;
		status.enrolled |= quest.status.enrolled;
		quest.status = status;
		if quest.status.value >= f64::from(quest.target) && !quest.status.completed {
			let final_status = api
				.quest_heartbeat(&quest, voice, true)
				.await
				.map_err(|e| e.label().to_owned())?;
			quest.status.completed |= final_status.completed;
			*list = api.quests().await.map_err(|e| e.label().to_owned())?;
			let server = find(list, id)?;
			quest.status.completed |= server.status.completed;
			quest.status.claimed |= server.status.claimed;
		}
		if quest.status.value <= before && !quest.status.completed {
			stalled += 1;
		} else {
			stalled = 0;
		}
		if let Some(q) = list.iter_mut().find(|q| q.id == id) {
			*q = quest.clone();
		}
		send_update(
			send,
			ctx,
			list,
			format!(
				"Running {}. Discord credited {:.0} / {}. Stop cancels further requests.",
				quest.name, quest.status.value, quest.target
			),
		)
		.await?;
		if stalled >= 5 {
			return Err("Discord reported no new progress for five checks. Stopped; no completion was assumed.".into());
		}
	}
	// Completion and claims are never inferred from a progress bar reaching its target.
	if flags.auto_claim && !quest.status.claimed {
		api.claim_quest(&quest)
			.await
			.map_err(|e| format!("Quest completed; reward claim failed: {}", e.label()))?;
		*list = api.quests().await.map_err(|e| e.label().to_owned())?;
		let claimed = find(list, id)?.status.claimed;
		send_update(
			send,
			ctx,
			list,
			"Quest completed; reward state reloaded.".into(),
		)
		.await?;
		if !claimed {
			return Ok(
				"Quest completed. Claim was sent, but reward confirmation is pending.".into(),
			);
		}
	}
	Ok("Quest completion confirmed by Discord.".into())
}
fn video_timestamp(initial: f64, elapsed: Duration, target: u32) -> f64 {
	(initial + elapsed.as_secs_f64()).min(f64::from(target))
}

#[cfg(test)]
mod tests {
	use super::*;
	struct Dropped(Option<tokio::sync::oneshot::Sender<()>>);
	impl Drop for Dropped {
		fn drop(&mut self) {
			if let Some(send) = self.0.take() {
				let _ = send.send(());
			}
		}
	}
	#[tokio::test]
	async fn account_replacement_and_voice_departure_abort_pending_jobs() {
		let secret =
			Arc::new(SessionSecret::from_owner_input("SYNTHETIC_QUEST_SESSION".into()).unwrap());
		let voice = VoiceContext {
			user: Id(1),
			channel: Id(2),
			guild: None,
		};
		let mut session = Session {
			secret: Some(secret),
			generation: 1,
			offline: false,
			proxy: watch::channel(Some(ApiProxy::Direct)).1,
			voice: Some(voice),
		};
		let mut view = Quests::default();
		view.observe_session(&session);
		for replace_account in [true, false] {
			let (started, ready) = tokio::sync::oneshot::channel();
			let (finished, released) = tokio::sync::oneshot::channel();
			let task = tokio::spawn(async move {
				let _drop = Dropped(Some(finished));
				let _ = started.send(());
				std::future::pending::<()>().await;
			});
			ready.await.unwrap();
			view.job = Some(Job {
				task,
				updates: mpsc::channel(1).1,
				voice: Some(voice),
			});
			if replace_account {
				session.generation += 1;
			} else {
				session.voice = None;
			}
			view.observe_session(&session);
			assert!(view.job.is_none());
			tokio::time::timeout(Duration::from_secs(2), released)
				.await
				.unwrap()
				.unwrap();
		}
	}
}
