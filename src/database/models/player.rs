use std::future::Future;

use mars_api_rs_macro::IdentifiableDocument;
use mars_api_rs_derive::IdentifiableDocument;
use mongodb::Collection;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use num_traits::ToPrimitive;

use crate::{database::CollectionOwner, socket::{leaderboard::ScoreType, player::{player_xp_listener::PlayerXPListener, player_events::PlayerXPGainData}, server::server_context::ServerContext, event_type::EventType}};
use crate::database::models::server::{ServerEvents, XPMultiplier};

use super::{punishment::StaffNote, level::LevelGamemode, r#match::Match};

#[derive(Debug, Serialize, Deserialize, Clone, IdentifiableDocument)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    #[id]
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub name_lower: String,
    pub last_session_id: Option<String>,
    pub first_joined_at: f64,
    pub last_joined_at: f64,
    pub ips: Vec<String>,
    pub notes: Vec<StaffNote>,
    pub rank_ids: Vec<String>,
    pub tag_ids: Vec<String>,
    pub active_tag_id: Option<String>,
    pub stats: PlayerStats,
    pub gamemode_stats: HashMap<LevelGamemode, GamemodeStats>,
    pub active_join_sound_id: Option<String>
}

impl Player {
    pub fn to_simple(&self) -> SimplePlayer {
        SimplePlayer { name: self.name.clone(), id: self.id.clone() }
    }

    pub fn id_name(&self) -> String {
        format!("{}/{}", self.id, self.name)
    }

    pub fn sanitized_copy(&self) -> Player {
        let mut clone = self.clone();
        clone.ips = Vec::new();
        clone.notes = Vec::new();
        clone.last_session_id = None;
        clone
    }

    pub async fn modify_gamemode_stats<F, Fut>(
        &mut self, 
        current_match: &Match, 
        modify: F
    ) where F: Fn(&mut GamemodeStats) -> Fut, Fut: Future<Output = ()> {
        let gamemodes = if !current_match.is_tracking_stats() { vec![LevelGamemode::Arcade] } else { current_match.level.gamemodes.clone() };
        for gamemode in gamemodes {
            modify(self.gamemode_stats.get_mut(&gamemode).unwrap_or(&mut GamemodeStats::default())).await;
        }
    }

    // TODO: Multipliers
    pub async fn add_xp(
        &mut self,
        server_context: &mut ServerContext,
        raw_xp: u32,
        reason: &String,
        notify: bool,
        raw_only: bool
    ) {
        let use_exponential = server_context.api_state.config.options.use_exponential_exp;
        let original_level = self.stats.get_level(use_exponential);
        let multiplier = match server_context.get_server_events().await {
            Some(events) =>  {
                match events.xp_multiplier {
                    Some(multiplier) => multiplier.value,
                    None => 1.0f32
                }
            }
            None => 1.0f32
        };
        let multiplied = ((raw_xp as f32) * multiplier).to_u32().unwrap_or(raw_xp);
        let target_xp_increment = if raw_only { multiplied } else { u32::max(PlayerXPListener::gain(raw_xp, original_level), multiplied) };
        self.stats.xp += target_xp_increment;
        let used_multiplier = target_xp_increment == multiplied;

            server_context.call(&EventType::PlayerXpGain, PlayerXPGainData {
                player_id: self.id.clone(), gain: target_xp_increment,
                reason: reason.clone(), notify, multiplier: if used_multiplier { Some(multiplier) } else { None }
            }).await;

        server_context.api_state.leaderboards.xp.increment(&self.id_name(), Some(target_xp_increment)).await;
    }
}

impl CollectionOwner<Player> for Player {
    fn get_collection(database: &crate::database::Database) -> &Collection<Player> { &database.players }
    fn get_collection_name() -> &'static str { "player" }
}

pub type GamemodeStats = PlayerStats;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStats {
    #[serde(default)]
    pub xp: u32,
    #[serde(default)]
    pub server_playtime: i64,
    #[serde(default)]
    pub game_playtime: u64,
    #[serde(default)]
    pub kills: u32,
    #[serde(default)]
    pub deaths: u32,
    #[serde(default)]
    pub void_kills: u32,
    #[serde(default)]
    pub void_deaths: u32,
    #[serde(default)]
    pub first_bloods: u32,
    #[serde(default)]
    pub first_bloods_suffered: u32,
    #[serde(default)]
    pub objectives: PlayerObjectiveStatistics,
    #[serde(default)]
    pub bow_shots_taken: u32,
    #[serde(default)]
    pub bow_shots_hit: u32,
    #[serde(default)]
    pub blocks_placed: HashMap<String, u32>,
    #[serde(default)]
    pub blocks_broken: HashMap<String, u32>,
    #[serde(default)]
    pub damage_taken: f64,
    #[serde(default)]
    pub damage_given: f64,
    #[serde(default)]
    pub damage_given_bow: f64,
    #[serde(default)]
    pub messages: PlayerMessages,
    #[serde(default)]
    pub wins: u32,
    #[serde(default)]
    pub losses: u32,
    #[serde(default)]
    pub ties: u32,
    #[serde(default)]
    pub matches: u32,
    #[serde(default)]
    pub matches_present_start: u32,
    #[serde(default)]
    pub matches_present_full: u32,
    #[serde(default)]
    pub matches_present_end: u32,
    #[serde(default)]
    pub records: PlayerRecords,
    #[serde(default)]
    pub weapon_kills: HashMap<String, u32>,
    #[serde(default)]
    pub weapon_deaths: HashMap<String, u32>,
    #[serde(default)]
    pub killstreaks: HashMap<String, u32>,
    #[serde(default)]
    pub killstreaks_ended: HashMap<String, u32>,
    #[serde(default)]
    pub achievements: HashMap<String, AchievementData>
}

impl PlayerStats {
    pub fn get_level(&self, use_exponential: bool) -> u32 {
        if use_exponential {
            ((0.0056f32 * (self.xp as f32).powf(0.715f32)) as u32) + 1
        } else {
            (self.xp + 5000) / 5000
        }
    }

    pub fn get_score(&self, score_type: &ScoreType) -> u32 {
        match score_type {
            ScoreType::Kills => self.kills,
            ScoreType::Deaths => self.deaths,
            ScoreType::FirstBloods => self.first_bloods,
            ScoreType::Wins => self.wins,
            ScoreType::Losses => self.losses,
            ScoreType::Ties => self.ties,
            ScoreType::Xp => self.xp,
            ScoreType::MessagesSent => self.messages.total(),
            ScoreType::MatchesPlayed => self.matches,
            // breaks 02/07/2106 05:28:15 AM UTC
            // u32 should be compatible w/ existing database because Java uses two's complement to
            // represent ints
            ScoreType::ServerPlaytime => u32::try_from(self.server_playtime).unwrap_or(u32::MAX),
            ScoreType::GamePlaytime => u32::try_from(self.game_playtime).unwrap_or(u32::MAX),
            ScoreType::CoreLeaks => self.objectives.core_leaks,
            ScoreType::CoreBlockDestroys => self.objectives.core_block_destroys,
            ScoreType::DestroyableDestroys => self.objectives.destroyable_destroys,
            ScoreType::DestroyableBlockDestroys => self.objectives.destroyable_block_destroys,
            ScoreType::FlagCaptures => self.objectives.flag_captures,
            ScoreType::FlagDrops => self.objectives.flag_drops,
            ScoreType::FlagPickups => self.objectives.flag_pickups,
            ScoreType::FlagDefends => self.objectives.flag_defends,
            ScoreType::FlagHoldTime => u32::try_from(self.objectives.total_flag_hold_time).unwrap_or(u32::MAX),
            ScoreType::WoolCaptures => self.objectives.wool_captures,
            ScoreType::WoolDrops => self.objectives.wool_drops,
            ScoreType::WoolPickups => self.objectives.wool_pickups,
            ScoreType::WoolDefends => self.objectives.wool_defends,
            ScoreType::ControlPointCaptures => self.objectives.control_point_captures,
            ScoreType::HighestKillstreak => {
                let key = self.killstreaks.keys().map(|ksstr| ksstr.parse::<u32>().unwrap_or(0))
                    .max().unwrap_or(100u32);
                let value = self.killstreaks.get(&key.to_string()).unwrap_or(&0).clone();
                value
            },
        }
    }
}

impl Default for PlayerStats {
    fn default() -> Self {
        PlayerStats {
            xp: 0,
            server_playtime: 0,
            game_playtime: 0,
            kills: 0,
            deaths: 0,
            void_kills: 0,
            void_deaths: 0,
            first_bloods: 0,
            first_bloods_suffered: 0,
            objectives: PlayerObjectiveStatistics::default(),
            bow_shots_taken: 0,
            bow_shots_hit: 0,
            blocks_placed: HashMap::new(),
            blocks_broken: HashMap::new(),
            damage_taken: 0.0,
            damage_given: 0.0,
            damage_given_bow: 0.0,
            messages: PlayerMessages::default(),
            wins: 0,
            losses: 0,
            ties: 0,
            matches: 0,
            matches_present_start: 0,
            matches_present_full: 0,
            matches_present_end: 0,
            records: PlayerRecords::default(),
            weapon_kills: HashMap::new(),
            weapon_deaths: HashMap::new(),
            killstreaks: HashMap::new(),
            killstreaks_ended: HashMap::new(),
            achievements: HashMap::new()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerObjectiveStatistics {
    pub core_leaks: u32,
    pub core_block_destroys: u32,
    pub destroyable_destroys: u32,
    pub destroyable_block_destroys: u32,
    pub flag_captures: u32,
    pub flag_pickups: u32,
    pub flag_drops: u32,
    pub flag_defends: u32,
    pub total_flag_hold_time: u64,
    pub wool_captures: u32,
    pub wool_drops: u32,
    pub wool_defends: u32,
    pub wool_pickups: u32,
    pub control_point_captures: u32
}

impl Default for PlayerObjectiveStatistics {
    fn default() -> Self {
        PlayerObjectiveStatistics {
            core_leaks: 0,
            core_block_destroys: 0,
            destroyable_destroys: 0,
            destroyable_block_destroys: 0,
            flag_captures: 0,
            flag_pickups: 0,
            flag_drops: 0,
            flag_defends: 0,
            total_flag_hold_time: 0,
            wool_captures: 0,
            wool_drops: 0,
            wool_defends: 0,
            wool_pickups: 0,
            control_point_captures: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRecords {
    #[serde(default)]
    pub longest_session: Option<SessionRecord>,
    #[serde(default)]
    pub longest_projectile_kill: Option<ProjectileRecord>,
    #[serde(default)]
    pub fastest_wool_capture: Option<PlayerRecord<u64>>,
    #[serde(default)]
    pub fastest_flag_capture: Option<PlayerRecord<u64>>,
    #[serde(default)]
    pub fastest_first_blood: Option<FirstBloodRecord>,
    #[serde(default)]
    pub kills_in_match: Option<PlayerRecord<u32>>,
    #[serde(default)]
    pub deaths_in_match: Option<PlayerRecord<u32>>
}

impl Default for PlayerRecords {
    fn default() -> Self {
        PlayerRecords {
            longest_session: None,
            longest_projectile_kill: None,
            fastest_wool_capture: None,
            fastest_flag_capture: None,
            fastest_first_blood: None,
            kills_in_match: None,
            deaths_in_match: None
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRecord<T> {
    pub match_id: String,
    pub player: SimplePlayer,
    pub value: T
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    pub length: u64
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectileRecord {
    pub match_id: String,
    pub player: SimplePlayer,
    pub distance: u32
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FirstBloodRecord {
    pub match_id: String,
    pub attacker: SimplePlayer,
    pub victim: SimplePlayer,
    pub time: u64
}

#[derive(Debug, Serialize, Deserialize, Clone, Hash, PartialEq, Eq)]
pub struct SimplePlayer {
    pub name: String,
    pub id: String
}

impl SimplePlayer {
    pub fn get_mini_icon_url(&self) -> String {
        format!("https://crafatar.com/avatars/{}?helm&size=50", self.id)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerMessages {
    #[serde(default)]
    pub staff: u32,
    #[serde(default)]
    pub global: u32,
    #[serde(default)]
    pub team: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AchievementData {
    pub completion_time: u64
}


impl PlayerMessages {
    pub fn total(&self) -> u32 {
        self.staff + self.global + self.team
    }
}

impl Default for PlayerMessages {
    fn default() -> Self {
        PlayerMessages { staff: 0, global: 0, team: 0 }
    }
}
