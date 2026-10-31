//! Bounded, unofficial normal-account quest routes. No game/process spoofing or OAuth grants.
use crate::{DiscordApi, Failure};
use model::Id;
use reqwest::Method;
use serde_json::{Value, json};

const MAX_LIST_BYTES: usize = 512 * 1024;
const MAX_ACTION_BYTES: usize = 64 * 1024;
const MAX_QUESTS: usize = 100;
pub const MAX_RUN_SECONDS: u32 = 25 * 60;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Task {
	Video,
	Activity,
	Achievement,
	PlayGame,
	StreamGame,
	Unsupported,
}
impl Task {
	pub fn label(self) -> &'static str {
		match self {
			Self::Video => "Watch video",
			Self::Activity => "Play activity",
			Self::Achievement => "Activity achievement",
			Self::PlayGame => "Play game on desktop",
			Self::StreamGame => "Stream game in voice",
			Self::Unsupported => "Requires the official client or game",
		}
	}
	pub fn needs_voice(self) -> bool {
		matches!(self, Self::Activity | Self::Achievement | Self::StreamGame)
	}
}

#[derive(Clone)]
pub struct Quest {
	pub id: Id,
	pub name: String,
	pub application: Option<Id>,
	pub task: Task,
	pub key: String,
	pub target: u32,
	pub status: Progress,
	pub expires: String,
	sealed: Option<String>,
}
#[derive(Clone, Copy, Default)]
pub struct Progress {
	pub value: f64,
	pub enrolled: bool,
	/// Only a service-provided completion timestamp confirms completion.
	pub completed: bool,
	pub claimed: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VoiceContext {
	pub user: Id,
	pub channel: Id,
	pub guild: Option<Id>,
}
impl VoiceContext {
	fn key(self) -> Result<String, Failure> {
		if self.user.0 == 0 || self.channel.0 == 0 || self.guild.is_some_and(|id| id.0 == 0) {
			return Err(Failure::Protocol);
		}
		Ok(match self.guild {
			Some(guild) => format!("guild:{guild}:{}:{}", self.channel, self.user),
			None => format!("call:{}:{}", self.channel, self.user),
		})
	}
}
fn field<'a>(v: &'a Value, snake: &str, camel: &str) -> &'a Value {
	v.get(snake)
		.or_else(|| v.get(camel))
		.unwrap_or(&Value::Null)
}
fn identifier(value: &Value) -> Result<Id, Failure> {
	let raw = value.as_str().ok_or(Failure::Protocol)?;
	let id = raw.parse::<Id>().map_err(|_| Failure::Protocol)?;
	if raw.len() > 20 || id.0 == 0 {
		return Err(Failure::Protocol);
	}
	Ok(id)
}
fn text(value: &Value, max: usize) -> Result<String, Failure> {
	let text = value.as_str().ok_or(Failure::Protocol)?;
	if text.len() > max || text.chars().any(char::is_control) {
		return Err(Failure::Capacity);
	}
	Ok(text.to_owned())
}
fn present(value: &Value) -> bool {
	value
		.as_str()
		.is_some_and(|s| !s.is_empty() && s.len() <= 64)
}
fn status(value: &Value, key: &str) -> Result<Progress, Failure> {
	let v = value
		.get("user_status")
		.or_else(|| value.get("userStatus"))
		.unwrap_or(value);
	let progress = v
		.get("progress")
		.and_then(|p| {
			p.get(key)
				.or_else(|| p.get("PLAY_ON_DESKTOP"))
				.or_else(|| p.get("PLAY_ON_DESKTOP_V2"))
				.or_else(|| p.get("STREAM_ON_DESKTOP"))
				.or_else(|| p.get("PLAY_ACTIVITY"))
				.or_else(|| p.get("WATCH_VIDEO"))
				.or_else(|| p.as_object().and_then(|obj| obj.values().next()))
		})
		.and_then(|p| p.get("value"));
	let value = match progress {
		None | Some(Value::Null) => 0.0,
		Some(n) => n
			.as_f64()
			.filter(|n| n.is_finite() && (0.0..=86_400.0).contains(n))
			.ok_or(Failure::Protocol)?,
	};
	Ok(Progress {
		value,
		enrolled: present(field(v, "enrolled_at", "enrolledAt")),
		completed: present(field(v, "completed_at", "completedAt")),
		claimed: present(field(v, "claimed_at", "claimedAt")),
	})
}
fn parse_list(bytes: &[u8]) -> Result<Vec<Quest>, Failure> {
	if bytes.len() > MAX_LIST_BYTES {
		return Err(Failure::Capacity);
	}
	let root: Value = serde_json::from_slice(bytes).map_err(|_| Failure::Protocol)?;
	let list = root
		.get("quests")
		.and_then(Value::as_array)
		.ok_or(Failure::Protocol)?;
	if list.len() > MAX_QUESTS {
		return Err(Failure::Capacity);
	}
	let mut output = Vec::with_capacity(list.len());
	for item in list {
		let id = identifier(&item["id"])?;
		if output.iter().any(|quest: &Quest| quest.id == id) {
			return Err(Failure::Protocol);
		}
		let config = &item["config"];
		let current = field(config, "task_config_v2", "taskConfigV2");
		let legacy = field(config, "task_config", "taskConfig");
		let tasks = current
			.get("tasks")
			.and_then(Value::as_object)
			.filter(|t| !t.is_empty())
			.or_else(|| legacy.get("tasks").and_then(Value::as_object))
			.ok_or(Failure::Protocol)?;
		if tasks.is_empty() || tasks.len() > 16 {
			return Err(Failure::Capacity);
		}
		let selected = [
			"WATCH_VIDEO",
			"PLAY_ON_DESKTOP",
			"PLAY_ON_DESKTOP_V2",
			"STREAM_ON_DESKTOP",
			"PLAY_ACTIVITY",
			"ACHIEVEMENT_IN_ACTIVITY",
		]
		.into_iter()
		.find_map(|key| tasks.get(key).map(|task| (key, task)))
		.or_else(|| tasks.iter().next().map(|(key, task)| (key.as_str(), task)))
		.ok_or(Failure::Protocol)?;
		let key = selected.0;
		if key.len() > 64 || !key.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
			return Err(Failure::Protocol);
		}
		let mut task = if key == "WATCH_VIDEO" || key.contains("VIDEO") {
			Task::Video
		} else if key == "PLAY_ACTIVITY" {
			Task::Activity
		} else if key == "ACHIEVEMENT_IN_ACTIVITY" {
			Task::Achievement
		} else if key.starts_with("STREAM") {
			Task::StreamGame
		} else if key.starts_with("PLAY") {
			Task::PlayGame
		} else {
			Task::Unsupported
		};
		let target = selected
			.1
			.get("target")
			.and_then(Value::as_u64)
			.and_then(|v| u32::try_from(v).ok())
			.filter(|v| *v > 0 && *v <= 86_400)
			.ok_or(Failure::Protocol)?;
		if target > MAX_RUN_SECONDS {
			task = Task::Unsupported;
		}
		let application = config
			.get("application")
			.and_then(|v| v.get("id"))
			.filter(|v| !v.is_null())
			.map(identifier)
			.transpose()?;
		if (task.needs_voice() || task == Task::PlayGame) && application.is_none() {
			task = Task::Unsupported;
		}
		let name = config
			.get("messages")
			.and_then(|v| v.get("quest_name").or_else(|| v.get("questName")))
			.or_else(|| config.get("application").and_then(|v| v.get("name")));
		let name = name
			.map(|v| text(v, 256))
			.transpose()?
			.filter(|v| !v.is_empty())
			.unwrap_or_else(|| "Discord quest".into());
		let sealed = field(item, "traffic_metadata_sealed", "trafficMetadataSealed");
		let sealed = (!sealed.is_null())
			.then(|| text(sealed, 4096))
			.transpose()?;
		let expires = field(config, "expires_at", "expiresAt");
		output.push(Quest {
			id,
			name,
			application,
			task,
			key: key.into(),
			target,
			status: status(field(item, "user_status", "userStatus"), key)?,
			expires: if expires.is_null() {
				String::new()
			} else {
				text(expires, 64)?
			},
			sealed,
		});
	}
	Ok(output)
}
impl DiscordApi {
	pub async fn quests(&self) -> Result<Vec<Quest>, Failure> {
		let bytes = self
			.request_limited(Method::GET, "/quests/@me", None, MAX_LIST_BYTES)
			.await?;
		parse_list(&bytes)
	}
	pub async fn enroll_quest(&self, quest: &Quest) -> Result<(), Failure> {
		self.request_limited(Method::POST,&format!("/quests/{}/enroll",quest.id),Some(json!({"location":11,"is_targeted":false,"metadata_sealed":null,"traffic_metadata_sealed":quest.sealed})),MAX_ACTION_BYTES).await?;
		Ok(())
	}
	pub async fn quest_video_progress(
		&self,
		quest: &Quest,
		timestamp: f64,
	) -> Result<Progress, Failure> {
		if quest.task != Task::Video
			|| !timestamp.is_finite()
			|| timestamp < 0.0
			|| timestamp > f64::from(quest.target)
		{
			return Err(Failure::Protocol);
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/quests/{}/video-progress", quest.id),
				Some(json!({"timestamp":timestamp})),
				MAX_ACTION_BYTES,
			)
			.await?;
		status(
			&serde_json::from_slice::<Value>(&bytes).map_err(|_| Failure::Protocol)?,
			&quest.key,
		)
	}
	pub async fn quest_heartbeat(
		&self,
		quest: &Quest,
		context: Option<VoiceContext>,
		terminal: bool,
	) -> Result<Progress, Failure> {
		if quest.task.needs_voice() && context.is_none() {
			return Err(Failure::Protocol);
		}
		let app = quest.application.ok_or(Failure::Protocol)?;
		let mut payload = serde_json::Map::new();
		payload.insert("application_id".into(), json!(app.to_string()));
		payload.insert("terminal".into(), json!(terminal));
		if let Some(ctx) = context {
			payload.insert("stream_key".into(), json!(ctx.key()?));
		}
		let bytes = self
			.request_limited(
				Method::POST,
				&format!("/quests/{}/heartbeat", quest.id),
				Some(Value::Object(payload)),
				MAX_ACTION_BYTES,
			)
			.await?;
		status(
			&serde_json::from_slice::<Value>(&bytes).map_err(|_| Failure::Protocol)?,
			&quest.key,
		)
	}
	pub async fn claim_quest(&self, quest: &Quest) -> Result<(), Failure> {
		if !quest.status.completed || quest.status.claimed {
			return Err(Failure::Protocol);
		}
		self.request_limited(Method::POST,&format!("/quests/{}/claim-reward",quest.id),Some(json!({"platform":0,"location":11,"is_targeted":false,"metadata_sealed":null,"traffic_metadata_sealed":quest.sealed})),MAX_ACTION_BYTES).await?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn fixture() -> Value {
		json!({"quests":[{"id":"1","config":{"application":{"id":"2","name":"Example"},"task_config":{"tasks":{"PLAY_ON_DESKTOP":{"target":100}}},"task_config_v2":{"tasks":{"WATCH_VIDEO":{"target":60}}}},"user_status":{"enrolled_at":"2026-10-31T00:00:00Z","progress":{"WATCH_VIDEO":{"value":60}}}}]})
	}
	#[test]
	fn current_tasks_win_and_progress_target_is_not_completion() {
		let list = parse_list(&serde_json::to_vec(&fixture()).unwrap()).unwrap();
		assert!(list[0].task == Task::Video);
		assert_eq!(list[0].status.value, 60.0);
		assert!(!list[0].status.completed);
		assert!(list[0].status.enrolled);
		let mut v = fixture();
		v["quests"][0]["user_status"]["completed_at"] = json!("2026-10-31T00:01:00Z");
		assert!(
			parse_list(&serde_json::to_vec(&v).unwrap()).unwrap()[0]
				.status
				.completed
		);
	}
	#[test]
	fn malformed_or_oversized_quests_are_rejected_and_stream_context_is_real() {
		for field in [json!(-1), json!(86_401), json!("60")] {
			let mut v = fixture();
			v["quests"][0]["user_status"]["progress"]["WATCH_VIDEO"]["value"] = field;
			assert!(parse_list(&serde_json::to_vec(&v).unwrap()).is_err());
		}
		let mut v = fixture();
		v["quests"] = json!(vec![v["quests"][0].clone(); 101]);
		assert!(matches!(
			parse_list(&serde_json::to_vec(&v).unwrap()),
			Err(Failure::Capacity)
		));
		assert!(parse_list(&vec![b' '; MAX_LIST_BYTES + 1]).is_err());
		assert_eq!(
			VoiceContext {
				user: Id(3),
				channel: Id(2),
				guild: Some(Id(1))
			}
			.key()
			.unwrap(),
			"guild:1:2:3"
		);
		assert!(
			VoiceContext {
				user: Id(0),
				channel: Id(2),
				guild: None
			}
			.key()
			.is_err()
		);
	}
}
