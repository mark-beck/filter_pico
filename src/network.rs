use cyw43::{Control, NetDriver};
use defmt::{info, warn, Format};
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};

use crate::ID;
use crate::messages;
use crate::messages::ForceState;
use crate::messages::Message;
use crate::messages::MessagePayload;
use crate::messages::Register;
use crate::FIRMWARE_VERSION;
use crate::STATE;
use crate::TOKEN;

use crate::state;
use crate::SERVER_IP;
use crate::SERVER_PORT;
use crate::WIFI_NETWORK;
use crate::WIFI_PASSWORD;

async fn join_network(control: &mut Control<'static>) -> bool {
    match control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD).await {
        Ok(()) => true,
        Err(err) => {
            info!("join failed with status={}", err.status);
            false
        }
    }
}

#[embassy_executor::task]
pub async fn start_network(
    mut control: Control<'static>,
    stack: &'static Stack<NetDriver<'static>>,
) -> ! {
    loop {
        // join wifi network
        while !join_network(&mut control).await {
            Timer::after(Duration::from_secs(1)).await;
        }

        // Wait for DHCP
        while !stack.is_config_up() {
            Timer::after(Duration::from_millis(100)).await;
        }
        let local_addr = stack.config_v4().unwrap().address.address();
        info!("IP address: {:?}", local_addr);

        let mut rx_buffer = [0; 4096];
        let mut tx_buffer = [0; 4096];

        let server_endpoint = embassy_net::IpEndpoint::new(SERVER_IP, SERVER_PORT);

        loop {
            let mut socket =
                embassy_net::tcp::TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
            // connect to server
            if let Err(e) = socket.connect(server_endpoint).await {
                warn!("connect error: {}", e);
                STATE.lock().await.network_state = state::NetworkState::Disconnected;
                Timer::after(Duration::from_secs(1)).await;
                continue;
            }
            let state = STATE.lock().await;
            let network_state = state.network_state;
            drop(state);
            match network_state {
                state::NetworkState::Disconnected => {
                    if let Err(e) = try_register(&mut socket).await {
                        warn!("register error: {}", e);
                        Timer::after(Duration::from_secs(1)).await;
                        continue;
                    }
                }
                state::NetworkState::Registered => {
                    if let Err(e) = try_heartbeat(&mut socket).await {
                        warn!("heartbeat error: {}", e);
                        STATE.lock().await.network_state = state::NetworkState::Disconnected;
                        Timer::after(Duration::from_secs(1)).await;
                        continue;
                    }
                }
            }
            Timer::after(Duration::from_secs(1)).await;
        }
    }
}

#[derive(Format)]
enum NetworkError {
    MessageError(&'static str),
    ReadError,
    WrongMessageType,
}

async fn try_heartbeat(socket: &mut TcpSocket<'_>) -> Result<(), NetworkError> {

    // create heartbeat message
    let state = STATE.lock().await;
    let heartbeat = messages::create_heartbeat(&state);
    drop(state);

    // send heartbeat message
    match send_message(socket, MessagePayload::Heartbeat(heartbeat)).await {
        Ok(()) => info!("sent heartbeat message"),
        Err(e) => {
            warn!("send error: {}", e);
            Timer::after(Duration::from_secs(1)).await;
            return Err(e);
        }
    }

    // read response
    let message = recv_message(socket).await?;
    if let MessagePayload::HeartbeatResponse(resp) = message.payload {
        info!("heartbeat response");
        info!("response: {}", resp);
        let mut state = STATE.lock().await;
        use messages::CommandType;
        match resp.command {
            CommandType::None => {},
            CommandType::ForceState(ForceState{state: 0, time}) => {
                info!("forced idle");
                state.state.filter_state = state::FilterState::ForcedIdle(time);
            },
            CommandType::ForceState(ForceState{state: 1, time}) => {
                info!("forced clean");
                state.state.filter_state = state::FilterState::ForcedClean(time);
            },
            CommandType::ForceState(ForceState{state: 2, time}) => {
                info!("forced fill");
                state.state.filter_state = state::FilterState::ForcedFill(time);
            },
            CommandType::ForceState(_) => {
                warn!("got invalid force state command");
            },
            CommandType::ResyncTime(time) => {
                info!("resync time");
                state.clock_skew = time.time - embassy_time::Instant::now().as_millis();
            },
            CommandType::UpdateConfig(conf) => {
                info!("got config update");
                state.config = state::Config {
                    waterlevel_fill_start: conf.waterlevel_fill_start,
                    waterlevel_fill_end: conf.waterlevel_fill_end,
                    clean_before_fill_duration: conf.clean_before_fill_duration,
                    clean_after_fill_duration: conf.clean_after_fill_duration,
                    leak_protection: conf.leak_protection == 1,
                };
            },
            CommandType::SetResetLeak(leak) => {
                if leak.leak == 1 {
                    info!("got set leak");
                    state.state.leak = Some(embassy_time::Instant::now().as_millis());
                } else {
                    info!("got reset leak");
                    state.state.leak = None;
                }
            },
            CommandType::ResetMeasurementError => {
                info!("got reset measurement error");
                state.state.measurement_error = None;
            },
            CommandType::NewFirmware(_) => warn!("got new firmware command: Unimplemented"),
            CommandType::ResetDevice => {
                info!("got reset device: Unimplemented");
            }
        }
    } else {
        warn!("wrong message type");
        return Err(NetworkError::WrongMessageType);
    }

    Ok(())
}

async fn try_register(socket: &mut TcpSocket<'_>) -> Result<(), NetworkError> {
    // get token
    let mut token = [0; 32];
    token.copy_from_slice(TOKEN.as_bytes());
    let mut id = [0; 32];
    id.copy_from_slice(ID.as_bytes());

    // create register message
    let register = Register {
        dev_id: id,
        token,
        dev_type: 0x01,
        firmware_version: FIRMWARE_VERSION,
        needs_config: 0x01,
    };

    // send register message
    match send_message(socket, MessagePayload::Register(register)).await {
        Ok(()) => info!("sent register message"),
        Err(e) => {
            warn!("send error: {}", e);
            Timer::after(Duration::from_secs(1)).await;
            return Err(e);
        }
    }

    // read response
    let message = recv_message(socket).await?;
    if let MessagePayload::Accepted(acc) = message.payload {
        info!("registration accepted");
        let mut state = STATE.lock().await;
        state.clock_skew = acc.time - embassy_time::Instant::now().as_millis();
        if let Some(conf) = acc.config {
            info!("got config while registering");
            state.config = state::Config {
                waterlevel_fill_start: conf.waterlevel_fill_start,
                waterlevel_fill_end: conf.waterlevel_fill_end,
                clean_before_fill_duration: conf.clean_before_fill_duration,
                clean_after_fill_duration: conf.clean_after_fill_duration,
                leak_protection: conf.leak_protection == 1,
            };
        }
        state.network_state = state::NetworkState::Registered;
    } else {
        warn!("wrong message type");
        return Err(NetworkError::WrongMessageType);
    }
    Ok(())
}

async fn recv_message(socket: &mut TcpSocket<'_>) -> Result<Message, NetworkError> {
    let mut buf = [0; 4096];
    match socket.read(&mut buf).await {
        Ok(n) => info!("read {} bytes", n),
        Err(e) => {
            warn!("read error: {}", e);
            return Err(NetworkError::ReadError);
        }
    }

    messages::decode_message(&buf).map_err(NetworkError::MessageError)
}

async fn send_message(
    socket: &mut TcpSocket<'_>,
    message: MessagePayload,
) -> Result<(), NetworkError> {
    let mut buf = [0; 4096];
    let (encoded_message, len) =
        messages::encode_message(message).map_err(NetworkError::MessageError)?;
    buf.copy_from_slice(&encoded_message);
    match socket.write(&buf[0..len]).await {
        Ok(n) => info!("wrote {} bytes", n),
        Err(e) => {
            warn!("write error: {}", e);
            return Err(NetworkError::ReadError);
        }
    }
    Ok(())
}
