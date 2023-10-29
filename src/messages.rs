use defmt::{info, Format};

use crate::state;

#[derive(Format)]
pub struct Message {
    header: MessageHeader,
    pub payload: MessagePayload,
    end: MessageEnd,
}

// size: 9 bytes
#[derive(Format)]
pub struct MessageHeader {
    magic: u32,
    typ: u8,
    length: u32,
}

#[derive(Format)]
pub enum MessagePayload {
    Register(Register),
    Accepted(Accepted),
    Heartbeat(Heartbeat),
    HeartbeatResponse(HeartbeatResponse),
}

// size: 68 bytes
#[derive(Format)]
pub struct Register {
    pub dev_id: [u8; 32],
    pub token: [u8; 32],
    pub dev_type: u8,
    pub firmware_version: u16,
    pub needs_config: u8,
}

// size: 9 bytes
#[derive(Format)]
pub struct Accepted {
    pub time: u64,
    pub config_following: u8,
    pub config: Option<Config>,
}

// size 33 bytes
#[derive(Format)]
pub struct Config {
    pub waterlevel_fill_start: u64,
    pub waterlevel_fill_end: u64,
    pub clean_before_fill_duration: u64,
    pub clean_after_fill_duration: u64,
    pub leak_protection: u8,
}

// size: 87 bytes
#[derive(Format)]
pub struct Heartbeat {
    pub dev_id: [u8; 32],
    pub dev_time: u64,
    pub filter_state: u8,
    pub forced_time_left: u64,
    pub last_state_change: u64,
    pub waterlevel: u64,
    pub measurement_error: u8,
    pub measurement_error_occured: u64,
    pub measurement_error_count: u32,
    pub leak: u8,
    pub leak_occured: u64,
}

// size: 1 byte
#[derive(Format)]
pub struct HeartbeatResponse {
    pub command_type: u8,
    pub command: CommandType,
}

#[derive(Format)]
pub enum CommandType {
    None,
    ForceState(ForceState),
    ResyncTime(ResyncTime),
    UpdateConfig(Config),
    SetResetLeak(SetResetLeak),
    ResetMeasurementError,
    NewFirmware(NewFirmware),
    ResetDevice,
}

// size: 9 bytes
#[derive(Format)]
pub struct ForceState {
    pub state: u8,
    pub time: u64,
}

// size: 8 bytes
#[derive(Format)]
pub struct ResyncTime {
    pub time: u64,
}

// size: 1 byte
#[derive(Format)]
pub struct SetResetLeak {
    pub leak: u8,
}

// size 10 bytes
#[derive(Format)]
pub struct NewFirmware {
    version: u16,
    size: u64,
}

// size: 1 byte
#[derive(Format)]
struct MessageEnd {
    xor: u8,
}

pub fn create_heartbeat(state: &state::Context) -> Heartbeat {
    let current_time = embassy_time::Instant::now().as_millis();
    let mut id = [0x01; 32];
    id.copy_from_slice(crate::ID.as_bytes());
    Heartbeat {
        dev_id: id,
        dev_time: current_time + state.clock_skew,
        filter_state: match state.state.filter_state {
            state::FilterState::Idle => 0x00,
            state::FilterState::CleanBeforeFill => 0x01,
            state::FilterState::CleanAfterFill => 0x02,
            state::FilterState::Fill => 0x03,
            state::FilterState::ForcedFill(_) => 0x04,
            state::FilterState::ForcedClean(_) => 0x05,
            state::FilterState::ForcedIdle(_) => 0x06,
        },
        forced_time_left: match state.state.filter_state {
            state::FilterState::ForcedFill(time) => time.saturating_sub(current_time - state.state.last_state_change),
            state::FilterState::ForcedClean(time) => time.saturating_sub(current_time - state.state.last_state_change),
            state::FilterState::ForcedIdle(time) => time.saturating_sub(current_time - state.state.last_state_change),
            _ => 0,
        },
        last_state_change: state.state.last_state_change + state.clock_skew,
        waterlevel: state.state.waterlevel.unwrap_or(0),
        measurement_error: u8::from(state.state.measurement_error.unwrap_or(0) != 0),
        measurement_error_occured: 0, // TODO: implement
        measurement_error_count: state
            .state
            .measurement_error
            .unwrap_or(0)
            .try_into()
            .unwrap_or(0),
        leak: u8::from(state.state.leak.is_some()),
        leak_occured: state.state.leak.map(|t| t  + state.clock_skew).unwrap_or(0),
    }
}

pub fn encode_message(message: MessagePayload) -> Result<([u8; 4096], usize), &'static str> {
    let bytes = match message {
        MessagePayload::Register(register) => encode_register_message(&register),
        MessagePayload::Heartbeat(heartbeat) => encode_heartbeat_message(&heartbeat),
        _ => return Err("wrong message type"),
    };

    Ok(bytes)
}

// buffer size: header: 9
fn encode_header(header: &MessageHeader) -> [u8; 9] {
    let mut buffer = [0; 9];
    buffer[0..4].copy_from_slice(&header.magic.to_be_bytes());
    buffer[4] = header.typ;
    buffer[5..9].copy_from_slice(&header.length.to_be_bytes());

    buffer
}

// buffer size: hearbeat: 87
fn encode_heartbeat(heartbeat: &Heartbeat) -> [u8; 87] {
    let mut buffer = [0; 87];
    buffer[0..32].copy_from_slice(&heartbeat.dev_id);
    buffer[32..40].copy_from_slice(&heartbeat.dev_time.to_be_bytes());
    buffer[40] = heartbeat.filter_state;
    buffer[41..49].copy_from_slice(&heartbeat.forced_time_left.to_be_bytes());
    buffer[49..57].copy_from_slice(&heartbeat.last_state_change.to_be_bytes());
    buffer[57..65].copy_from_slice(&heartbeat.waterlevel.to_be_bytes());
    buffer[65] = heartbeat.measurement_error;
    buffer[66..74].copy_from_slice(&heartbeat.measurement_error_occured.to_be_bytes());
    buffer[74..78].copy_from_slice(&heartbeat.measurement_error_count.to_be_bytes());
    buffer[78] = heartbeat.leak;
    buffer[79..87].copy_from_slice(&heartbeat.leak_occured.to_be_bytes());

    buffer
}

fn encode_heartbeat_message(heartbeat: &Heartbeat) -> ([u8; 4096], usize) {
    let mut buffer = [0; 4096];
    buffer[0..9].copy_from_slice(&encode_header(&MessageHeader {
        magic: 0xfafafaff,
        typ: 0x03,
        length: 97,
    }));

    buffer[9..96].copy_from_slice(&encode_heartbeat(heartbeat));
    buffer[96] = 0;

    (buffer, 97)
}

// buffer size: register: 68
fn encode_register(register: &Register) -> [u8; 68] {
    let mut buffer = [0; 68];
    buffer[0..32].copy_from_slice(&register.dev_id);
    buffer[32..64].copy_from_slice(&register.token);
    buffer[64] = register.dev_type;
    buffer[65..67].copy_from_slice(&register.firmware_version.to_be_bytes());
    buffer[67] = register.needs_config;

    buffer
}

pub fn encode_register_message(register: &Register) -> ([u8; 4096], usize) {
    let mut buffer = [0; 4096];
    buffer[0..9].copy_from_slice(&encode_header(&MessageHeader {
        magic: 0xfafafaff,
        typ: 0x01,
        length: 78,
    }));

    buffer[9..77].copy_from_slice(&encode_register(register));
    buffer[77] = 0;

    (buffer, 78)
}

pub fn decode_message(buffer: &[u8]) -> Result<Message, &'static str> {
    let header = decode_header(&buffer[0..9])?;

    // can't use buffer.len() bc buffer has set size of 4096
    // if header.length != buffer.len() as u32 {
    //     return Err("length does not match");
    // }
    let payload = decode_payload(&buffer[9..buffer.len() - 1], header.typ)?;
    let end = decode_end(&buffer[buffer.len() - 1..buffer.len()]);

    Ok(Message {
        header,
        payload,
        end,
    })
}

fn decode_header(buffer: &[u8]) -> Result<MessageHeader, &'static str> {
    let magic = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    info!("magic: {}", magic);
    if magic != 0xfafafaff {
        return Err("magic does not match");
    }
    let typ = buffer[4];
    if !(1..=4).contains(&typ) {
        return Err("typ does not match");
    }
    let length = u32::from_be_bytes([buffer[5], buffer[6], buffer[7], buffer[8]]);

    Ok(MessageHeader { magic, typ, length })
}

fn decode_payload(buffer: &[u8], typ: u8) -> Result<MessagePayload, &'static str> {
    Ok(match typ {
        2 => MessagePayload::Accepted(decode_accepted(buffer)),
        4 => MessagePayload::HeartbeatResponse(decode_heartbeat_response(buffer)),
        _ => return Err("typ does not match2"),
    })
}

fn decode_end(buffer: &[u8]) -> MessageEnd {
    MessageEnd { xor: buffer[0] }
}

fn decode_accepted(buffer: &[u8]) -> Accepted {
    let time = u64::from_be_bytes([
        buffer[0], buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7],
    ]);
    let config_following = buffer[8];

    let config = if config_following == 1 {
        Some(decode_config(&buffer[9..buffer.len() - 1]))
    } else {
        None
    };

    Accepted {
        time,
        config_following,
        config,
    }
}

const fn decode_config(buffer: &[u8]) -> Config {
    let waterlevel_fill_start = u64::from_be_bytes([
        buffer[0], buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7],
    ]);
    let waterlevel_fill_end = u64::from_be_bytes([
        buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13], buffer[14],
        buffer[15],
    ]);
    let clean_before_fill_duration = u64::from_be_bytes([
        buffer[16], buffer[17], buffer[18], buffer[19], buffer[20], buffer[21], buffer[22],
        buffer[23],
    ]);
    let clean_after_fill_duration = u64::from_be_bytes([
        buffer[24], buffer[25], buffer[26], buffer[27], buffer[28], buffer[29], buffer[30],
        buffer[31],
    ]);
    let leak_protection = buffer[32];

    Config {
        waterlevel_fill_start,
        waterlevel_fill_end,
        clean_before_fill_duration,
        clean_after_fill_duration,
        leak_protection,
    }
}

fn decode_heartbeat_response(buffer: &[u8]) -> HeartbeatResponse {
    let command_type = buffer[0];
    let command = match command_type {
        0 => CommandType::None,
        1 => CommandType::ForceState(decode_force_state(&buffer[1..buffer.len() - 1])),
        2 => CommandType::ResyncTime(decode_resync_time(&buffer[1..buffer.len() - 1])),
        3 => CommandType::UpdateConfig(decode_config(&buffer[1..buffer.len() - 1])),
        4 => CommandType::SetResetLeak(decode_set_reset_leak(&buffer[1..buffer.len() - 1])),
        5 => CommandType::ResetMeasurementError,
        6 => CommandType::NewFirmware(decode_new_firmware(&buffer[1..buffer.len() - 1])),
        7 => CommandType::ResetDevice,
        _ => {
            return HeartbeatResponse {
                command_type,
                command: CommandType::None,
            }
        }
    };

    HeartbeatResponse {
        command_type,
        command,
    }
}

const fn decode_force_state(buffer: &[u8]) -> ForceState {
    let state = buffer[0];
    let time = u64::from_be_bytes([
        buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7], buffer[8],
    ]);

    ForceState { state, time }
}

const fn decode_resync_time(buffer: &[u8]) -> ResyncTime {
    let time = u64::from_be_bytes([
        buffer[0], buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7],
    ]);

    ResyncTime { time }
}

const fn decode_set_reset_leak(buffer: &[u8]) -> SetResetLeak {
    let leak = buffer[0];

    SetResetLeak { leak }
}

const fn decode_new_firmware(buffer: &[u8]) -> NewFirmware {
    let version = u16::from_be_bytes([buffer[0], buffer[1]]);
    let size = u64::from_be_bytes([
        buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7], buffer[8], buffer[9],
    ]);

    NewFirmware { version, size }
}
