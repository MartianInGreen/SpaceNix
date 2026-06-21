use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view};

use crate::device::device as _;
use crate::user::{require_registered_user, session__view as _};

#[spacetimedb::table(accessor = ui_event, public, event)]
pub struct UiEvent {
    pub owner: Identity,
    pub target_device_id: Option<u64>,
    pub command_id: Option<u64>,
    pub kind: String,
    pub payload_json: String,
    pub created_at: Timestamp,
}

#[spacetimedb::table(accessor = ui_command, public)]
pub struct UiCommand {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    pub target_device_id: Option<u64>,
    pub kind: String,
    pub payload_json: String,
    pub created_at: Timestamp,
    pub handled_at: Option<Timestamp>,
    pub handled_by_device_id: Option<u64>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct UiCommandMetadata {
    pub id: u64,
    pub owner: Identity,
    pub target_device_id: Option<u64>,
    pub kind: String,
    pub payload_json: String,
    pub created_at: Timestamp,
    pub handled_at: Option<Timestamp>,
    pub handled_by_device_id: Option<u64>,
}

impl From<UiCommand> for UiCommandMetadata {
    fn from(c: UiCommand) -> Self {
        Self {
            id: c.id,
            owner: c.owner,
            target_device_id: c.target_device_id,
            kind: c.kind,
            payload_json: c.payload_json,
            created_at: c.created_at,
            handled_at: c.handled_at,
            handled_by_device_id: c.handled_by_device_id,
        }
    }
}

fn validate_kind(kind: String) -> Result<String, String> {
    let kind = kind.trim().to_string();
    if kind.is_empty() {
        return Err("event kind cannot be empty".to_string());
    }
    if kind.len() > 128 {
        return Err("event kind must be 128 characters or fewer".to_string());
    }
    if !kind
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-'))
    {
        return Err("event kind contains invalid characters".to_string());
    }
    Ok(kind)
}

fn validate_payload(payload_json: String) -> Result<String, String> {
    if payload_json.len() > 16 * 1024 {
        return Err("event payload must be 16 KiB or smaller".to_string());
    }
    if payload_json.chars().any(|c| c == '\0') {
        return Err("event payload cannot contain NUL".to_string());
    }
    Ok(payload_json)
}

fn validate_target_device(
    ctx: &ReducerContext,
    owner: Identity,
    target_device_id: Option<u64>,
) -> Result<Option<u64>, String> {
    let Some(device_id) = target_device_id else {
        return Ok(None);
    };
    let device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "target device not found".to_string())?;
    if device.owner != owner {
        return Err("target device is not yours".to_string());
    }
    Ok(Some(device_id))
}

#[spacetimedb::reducer]
pub fn send_ui_event(
    ctx: &ReducerContext,
    target_device_id: Option<u64>,
    kind: String,
    payload_json: String,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let kind = validate_kind(kind)?;
    let payload_json = validate_payload(payload_json)?;
    let target_device_id = validate_target_device(ctx, user.identity, target_device_id)?;

    let command = ctx.db.ui_command().insert(UiCommand {
        id: 0,
        owner: user.identity,
        target_device_id,
        kind: kind.clone(),
        payload_json: payload_json.clone(),
        created_at: ctx.timestamp,
        handled_at: None,
        handled_by_device_id: None,
    });

    ctx.db.ui_event().insert(UiEvent {
        owner: user.identity,
        target_device_id,
        command_id: Some(command.id),
        kind,
        payload_json,
        created_at: ctx.timestamp,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn ack_ui_command(
    ctx: &ReducerContext,
    command_id: u64,
    device_id: u64,
) -> Result<(), String> {
    let user = require_registered_user(ctx)?;
    let device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != user.identity {
        return Err("not your device".to_string());
    }
    let mut command = ctx
        .db
        .ui_command()
        .id()
        .find(command_id)
        .ok_or_else(|| "command not found".to_string())?;
    if command.owner != user.identity {
        return Err("not your command".to_string());
    }
    if command
        .target_device_id
        .is_some_and(|target| target != device_id)
    {
        return Err("command targets another device".to_string());
    }
    command.handled_at = Some(ctx.timestamp);
    command.handled_by_device_id = Some(device_id);
    ctx.db.ui_command().id().update(command);
    Ok(())
}

#[view(accessor = my_ui_commands, public)]
fn my_ui_commands(ctx: &ViewContext) -> Vec<UiCommandMetadata> {
    let Some(user) = ctx.db.session().connection().find(ctx.sender()).map(|s| s.user) else {
        return Vec::new();
    };
    ctx.db
        .ui_command()
        .owner()
        .filter(user)
        .map(UiCommandMetadata::from)
        .collect()
}
