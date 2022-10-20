use std::ops::{Deref, DerefMut};
use std::thread::{sleep, sleep_ms};
use crate::constant::{ssh_msg_code};
use crate::error::{SshError, SshResult};
use crate::data::Data;
use crate::slog::log;
use crate::channel_exec::ChannelExec;
use crate::channel_scp::ChannelScp;
use crate::channel_shell::ChannelShell;
use crate::h::H;
use crate::{kex, Session};
use crate::window_size::WindowSize;

pub struct Channel {
    pub(crate) remote_close: bool,
    pub(crate) local_close: bool,
    pub(crate) window_size: WindowSize,
    pub(crate) session: *mut Session,
}

impl Deref for Channel {
    type Target = WindowSize;

    fn deref(&self) -> &Self::Target {
        &self.window_size
    }
}

impl DerefMut for Channel {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.window_size
    }
}

impl Channel {
    pub(crate) fn other(&mut self, message_code: u8, mut result: Data) -> SshResult<()> {
        match message_code {
            ssh_msg_code::SSH_MSG_GLOBAL_REQUEST => {
                let mut data = Data::new();
                data.put_u8(ssh_msg_code::SSH_MSG_REQUEST_FAILURE);
                let session = self.get_session_mut();
                session.client.as_mut().unwrap().write(data)?;
            }
            ssh_msg_code::SSH_MSG_KEXINIT => {
                // TODO read 1GB
                let mut h = H::new();
                let session = self.get_session_mut();
                let client = session.client.as_mut().unwrap();
                let cv = client.config.version.client_version.as_str();
                let sv = client.config.version.server_version.as_str();
                h.set_v_c(cv);
                h.set_v_s(sv);
                log::info!("receive server algorithm list.");
                let mut data = vec![];
                data.push(message_code);
                data.extend(result.as_slice());
                let data = Data::from(data);
                h.set_i_s(data.clone().as_slice());
                kex::processing_server_algorithm(&mut client.config, data)?;

                sleep_ms(10000000);
                // log::info!("send client algorithm list.");
                // kex::send_algorithm(&mut h, client)?;
                // let mut key_exchange = client.config.algorithm.matching_key_exchange_algorithm()?;
                // let mut public_key = client.config.algorithm.matching_public_key_algorithm()?;
                // log::info!("send qc.");
                // kex::send_qc(client, key_exchange.get_public_key())?;
                // log::info!("signature verification.");
                // let session_id = kex::verify_signature_and_new_keys(client, &mut public_key, &mut key_exchange, &mut h)?;
                // kex::key_agreement(&mut h, client)?;
            }
            // 通道大小
            ssh_msg_code::SSH_MSG_CHANNEL_WINDOW_ADJUST => {
                // 接收方通道号， 暂时不需要
                result.get_u32();
                // 需要调整增加的窗口大小
                let rws = result.get_u32();
                self.window_size.add_remote_window_size(rws);
                self.window_size.add_remote_max_window_size(rws);
            },
            ssh_msg_code::SSH_MSG_CHANNEL_EOF => {}
            ssh_msg_code::SSH_MSG_CHANNEL_REQUEST => {}
            ssh_msg_code::SSH_MSG_CHANNEL_SUCCESS => {}
            ssh_msg_code::SSH_MSG_CHANNEL_FAILURE => return Err(SshError::from("channel failure.")),
            ssh_msg_code::SSH_MSG_CHANNEL_CLOSE => {
                let cc = result.get_u32();
                if cc == self.client_channel_no {
                    self.remote_close = true;
                    self.close()?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn open_shell(self) -> SshResult<ChannelShell> {
        log::info!("shell opened.");
        return ChannelShell::open(self)
    }

    pub fn open_exec(self) -> SshResult<ChannelExec> {
        log::info!("exec opened.");
        return Ok(ChannelExec::open(self))
    }

    pub fn open_scp(self) -> SshResult<ChannelScp> {
        log::info!("scp opened.");
        return Ok(ChannelScp::open(self))
    }

    pub fn close(&mut self) -> SshResult<()> {
        log::info!("channel close.");
        self.send_close()?;
        self.receive_close()
    }

    fn send_close(&mut self) -> SshResult<()> {
        if self.local_close { return Ok(()); }
        let mut data = Data::new();
        data.put_u8(ssh_msg_code::SSH_MSG_CHANNEL_CLOSE)
            .put_u32(self.server_channel_no);
        let session = self.get_session_mut();
        session.client.as_mut().unwrap().write(data)?;
        self.local_close = true;
        Ok(())
    }

    fn receive_close(&mut self) -> SshResult<()> {
        if self.remote_close { return Ok(()); }
        loop {
            // close 时不消耗窗口空间
            let results = {
                self.get_session_mut().client.as_mut().unwrap().read()
            }?;
            for mut result in results {
                if result.is_empty() { continue }
                let message_code = result.get_u8();
                match message_code {
                    ssh_msg_code::SSH_MSG_CHANNEL_CLOSE => {
                        let cc = result.get_u32();
                        if cc == self.client_channel_no {
                            self.remote_close = true;
                            return Ok(())
                        }
                    }
                    _ => self.other(message_code, result)?
                }
            }
        }
    }

    pub(crate) fn get_session_mut(&self) -> &mut Session {
        unsafe { &mut *self.session }
    }

    pub(crate) fn is_close(&self) -> bool {
        self.local_close && self.remote_close
    }
}
