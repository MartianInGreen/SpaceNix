use spacetimedb::{
    Identity, ReducerContext, SpacetimeType, Table, Timestamp, ViewContext, view,
};

#[spacetimedb::table(accessor = device, public)]
pub struct Device {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub owner: Identity,
    pub name: String,
    pub hostname: Option<String>,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
}

#[derive(SpacetimeType, Clone, Debug)]
pub struct DeviceMetadata {
    pub id: u64,
    pub owner: Identity,
    pub name: String,
    pub hostname: Option<String>,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
}

impl From<Device> for DeviceMetadata {
    fn from(d: Device) -> Self {
        Self {
            id: d.id,
            owner: d.owner,
            name: d.name,
            hostname: d.hostname,
            created_at: d.created_at,
            last_seen_at: d.last_seen_at,
        }
    }
}

fn validate_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.len() > 128 {
        return Err("name must be 128 characters or fewer".to_string());
    }
    Ok(name)
}

fn normalize_hostname(hostname: Option<String>) -> Result<Option<String>, String> {
    let Some(h) = hostname else {
        return Ok(None);
    };
    let h = h.trim().to_string();
    if h.is_empty() {
        return Ok(None);
    }
    if h.len() > 256 {
        return Err("hostname must be 256 characters or fewer".to_string());
    }
    Ok(Some(h))
}

#[spacetimedb::reducer]
pub fn register_device(
    ctx: &ReducerContext,
    name: String,
    hostname: Option<String>,
) -> Result<(), String> {
    let name = validate_name(name)?;
    let hostname = normalize_hostname(hostname)?;
    ctx.db.device().insert(Device {
        id: 0,
        owner: ctx.sender(),
        name,
        hostname,
        created_at: ctx.timestamp,
        last_seen_at: None,
    });
    Ok(())
}

#[spacetimedb::reducer]
pub fn rename_device(
    ctx: &ReducerContext,
    device_id: u64,
    name: String,
) -> Result<(), String> {
    let name = validate_name(name)?;
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != ctx.sender() {
        return Err("not your device".to_string());
    }
    device.name = name;
    ctx.db.device().id().update(device);
    Ok(())
}

#[spacetimedb::reducer]
pub fn set_device_hostname(
    ctx: &ReducerContext,
    device_id: u64,
    hostname: Option<String>,
) -> Result<(), String> {
    let hostname = normalize_hostname(hostname)?;
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != ctx.sender() {
        return Err("not your device".to_string());
    }
    device.hostname = hostname;
    ctx.db.device().id().update(device);
    Ok(())
}

#[spacetimedb::reducer]
pub fn touch_device(ctx: &ReducerContext, device_id: u64) -> Result<(), String> {
    let mut device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != ctx.sender() {
        return Err("not your device".to_string());
    }
    device.last_seen_at = Some(ctx.timestamp);
    ctx.db.device().id().update(device);
    Ok(())
}

#[spacetimedb::reducer]
pub fn delete_device(ctx: &ReducerContext, device_id: u64) -> Result<(), String> {
    let device = ctx
        .db
        .device()
        .id()
        .find(device_id)
        .ok_or_else(|| "device not found".to_string())?;
    if device.owner != ctx.sender() {
        return Err("not your device".to_string());
    }
    ctx.db.device().id().delete(device_id);
    Ok(())
}

#[view(accessor = my_devices, public)]
fn my_devices(ctx: &ViewContext) -> Vec<DeviceMetadata> {
    ctx.db
        .device()
        .owner()
        .filter(ctx.sender())
        .map(DeviceMetadata::from)
        .collect()
}
