# Message protocol

1. The decice sends a message to the server to register its id and ask for a config

2. The server sends a accepded message to the device with the config if requested

3. the device starts sending heartbeat messages with the current state to the server

4. the server can respond to the heartbeat with a command or config update

## Message Header

| Field | Size | Description |
| --- | --- | --- |
| Magic | 4 bytes | 0xfafafaff |
| Type | 1 byte | 0x01: Register, 0x02: Accepted, 0x03: Heartbeat, 0x04: HeartbeatResponse |
| Length | 4 bytes | Length of the payload |
| Payload | variable | Payload |

## Payload

### Register

| Field | Size | Description |
| --- | --- | --- |
| dev_id | 32 bytes | |
| token | 32 bytes | |
| dev_type | 1 byte | always 0x01 |
| firmware_version | 2 byte | |
| needs_config | 1 byte | 0x00: no, 0x01: yes |

### Accepted

| Field | Size | Description |
| --- | --- | --- |
| time | 8 bytes | ms since epoch |
| config_following | 1 byte | 0x00: no, 0x01: yes |

### Config

| Field | Size | Description |
| --- | --- | --- |
| waterlevel_fill_start | 8 byte | t |
| waterlevel_fill_end | 8 byte |  |
| clean_before_fill_duration | 8 byte |  |
| clean_after_fill_duration | 8 byte |  |
| leak_protection | 1 byte | 0x00: no, 0x01: yes |

### Heartbeat

| Field | Size | Description |
| --- | --- | --- |
| dev_id | 32 bytes | Device ID |
| dev_time | 8 bytes | Device time in ms since epoch |
| filter_state | 1 byte | 0x00: Idle, 0x01: CleanBeforeFill, 0x02: CleanAfterFill, 0x03: Fill, 0x04: ForcedFill, 0x05: ForcedClean, 0x06: ForcedIdle |
| forced_time_left | 8 byte | Forced state time left in ms |
| last_state_change | 8 byte | Last state change ms since epoch |
| waterlevel | 8 byte | Waterlevel mm from Sensor |
| measurement_error | 1 byte | 0x00: no, 0x01: yes |
| measurement_error_occured | 8 byte | last time measurement error occured ms since epoch |
| measurement_error_count | 4 byte | number of measurement errors since last reset |
| leak | 1 byte | 0x00: no, 0x01: yes |
| leak_occured | 8 byte | first time leak occured ms since epoch |

### Heartbeat Response

| Field | Size | Description |
| --- | --- | --- |
| command_type | 1 byte | 0x00: no command, 0x01: force state, 0x02: resync time, 0x03: update config, 0x04 set/reset leak, 0x05: reset measurement error, 0x06: load new firmware, 0x07: reset device |
| command_payload | variable | |

## Command

### Force State

| Field | Size | Description |
| --- | --- | --- |
| state | 1 byte | 0x00: ForcedIdle, 0x01: ForcedClean, 0x02: ForcedFill |
| time | 8 byte | time in ms to force state |

### Resync Time

| Field | Size | Description |
| --- | --- | --- |
| time | 8 byte | time in ms since epoch |

### Update Config

see config

### Set/Reset Leak

| Field | Size | Description |
| --- | --- | --- |
| leak | 1 byte | 0x00: no, 0x01: yes |

### Reset Measurement Error

no payload

### Load new Firmware

| Field | Size | Description |
| --- | --- | --- |
| firmware_version | 2 byte | |
| firmware_size | 8 byte | |
| firmware | variable | |

### Reset Device

no payload

## Message End

| Field | Size | Description |
| --- | --- | --- |
| Checksum | 1 byte | XOR of all bytes in the message |

