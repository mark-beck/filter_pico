use cyw43::{Control, NetDriver};
use defmt::*;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};

use crate::FIRMWARE_VERSION;
use crate::TOKEN;
use crate::messages;
use crate::messages::Register;

use crate::WIFI_NETWORK;
use crate::WIFI_PASSWORD;
use crate::SERVER_IP;
use crate::SERVER_PORT;

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
pub async fn start_network(mut control: Control<'static>, stack: &'static Stack<NetDriver<'static>>) -> ! {
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
            let mut socket = embassy_net::tcp::TcpSocket::new(&stack, &mut rx_buffer, &mut tx_buffer);
            // connect to server
            if let Err(e) = socket.connect(server_endpoint).await {
                warn!("connect error: {}", e);
                Timer::after(Duration::from_secs(1)).await;
                continue;
            }
            loop {

                let mut token = [0; 32];
                token.copy_from_slice(TOKEN.as_bytes());

                // send Register Message
                let register = Register {
                    dev_id: [0x01; 32],
                    token,
                    dev_type: 0x01,
                    firmware_version: FIRMWARE_VERSION,
                    needs_config: 0x01,
                };

                let register_message = messages::encode_register_message(register);

                if let Err(e) = socket.write(&register_message).await {
                    warn!("write error: {}", e);
                    Timer::after(Duration::from_secs(1)).await;
                    break;
                }

                // read response

                let mut buf = [0; 4096];
                match socket.read(&mut buf).await.map_err(|e| error!("{:?}", e)) {
                    Ok(n) => info!("read {} bytes", n),
                    Err(e) => {
                        warn!("read error: {}", e);
                        continue;
                    }
                }

                let message = messages::decode_message(&buf);
            }
            Timer::after(Duration::from_secs(1)).await;
        }
    }
}