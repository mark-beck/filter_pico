use cyw43::{Control, NetDriver};
use defmt::*;
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};

use crate::messages;
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
        Ok(_) => return true,
        Err(err) => {
            info!("join failed with status={}", err.status);
            return false;
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
        let mut _buf = [0; 4096];

        let server_endpoint = embassy_net::IpEndpoint::new(SERVER_IP, SERVER_PORT);

        loop {
            let mut socket =
                embassy_net::tcp::TcpSocket::new(&stack, &mut rx_buffer, &mut tx_buffer);
            // connect to server
            if let Err(e) = socket.connect(server_endpoint).await {
                warn!("connect error: {}", e);
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

async fn try_heartbeat(mut socket: &mut TcpSocket<'_>) -> Result<(), NetworkError> {
    // get token
    let mut token = [0; 32];
    token.copy_from_slice(TOKEN.as_bytes());

    let state = STATE.lock().await;

    // create heartbeat message
    let heartbeat = messages::create_heartbeat(&state);
    drop(state);

    // send heartbeat message
    match send_message(&mut socket, MessagePayload::Heartbeat(heartbeat)).await {
        Ok(_) => info!("sent heartbeat message"),
        Err(e) => {
            warn!("send error: {}", e);
            Timer::after(Duration::from_secs(1)).await;
            return Err(e);
        }
    }

    // read response
    let message = recv_message(&mut socket).await?;
    match message.payload {
        MessagePayload::HeartbeatResponse(resp) => {
            info!("heartbeat response");
            info!("response: {}", resp);
        }
        _ => {
            warn!("wrong message type");
            return Err(NetworkError::WrongMessageType);
        }
    }

    Ok(())
}

async fn try_register(mut socket: &mut TcpSocket<'_>) -> Result<(), NetworkError> {
    // get token
    let mut token = [0; 32];
    token.copy_from_slice(TOKEN.as_bytes());

    // create register message
    let register = Register {
        dev_id: [0x01; 32],
        token,
        dev_type: 0x01,
        firmware_version: FIRMWARE_VERSION,
        needs_config: 0x01,
    };

    // send register message
    match send_message(&mut socket, MessagePayload::Register(register)).await {
        Ok(_) => info!("sent register message"),
        Err(e) => {
            warn!("send error: {}", e);
            Timer::after(Duration::from_secs(1)).await;
            return Err(e);
        }
    }

    // read response
    let message = recv_message(&mut socket).await?;
    match message.payload {
        MessagePayload::Accepted(acc) => {
            if acc.config.is_none() {
                warn!("config not following");
                return Err(NetworkError::WrongMessageType);
            }
            info!("accepted");
            let conf = acc.config.unwrap();
            let mut state = STATE.lock().await;
            state.config = state::Config {
                waterlevel_fill_start: conf.waterlevel_fill_start,
                waterlevel_fill_end: conf.waterlevel_fill_end,
                clean_before_fill_duration: conf.clean_before_fill_duration,
                clean_after_fill_duration: conf.clean_after_fill_duration,
                leak_protection: conf.leak_protection == 1,
            };
            state.network_state = state::NetworkState::Registered;
        }
        _ => {
            warn!("wrong message type");
            return Err(NetworkError::WrongMessageType);
        }
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

    Ok(messages::decode_message(&buf).map_err(|s| NetworkError::MessageError(s))?)
}

async fn send_message(
    socket: &mut TcpSocket<'_>,
    message: MessagePayload,
) -> Result<(), NetworkError> {
    let mut buf = [0; 4096];
    let (encoded_message, len) =
        messages::encode_message(message).map_err(|s| NetworkError::MessageError(s))?;
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
