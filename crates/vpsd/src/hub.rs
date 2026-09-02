//! In-memory PTY table. Sessions outlive a client splice.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

use crate::pty::{self, Session};

pub struct Slot {
    pub session: Session,
    pub attached: bool,
}

pub struct Hub {
    next_id: u64,
    slots: HashMap<u64, Slot>,
    shell: String,
}

impl Hub {
    pub fn new(shell: String) -> Self {
        Self {
            next_id: 1,
            slots: HashMap::new(),
            shell,
        }
    }

    /// Reuse an idle living session, or spawn a new PTY.
    pub fn take_or_create(&mut self, cols: u16, rows: u16) -> std::io::Result<(u64, OwnedFd)> {
        self.reap();
        let ws = pty::winsize(cols, rows);
        let idle = self
            .slots
            .iter()
            .find(|(_, s)| !s.attached)
            .map(|(id, _)| *id);
        if let Some(id) = idle {
            let slot = self.slots.get_mut(&id).expect("idle id");
            slot.attached = true;
            let _ = pty::set_winsize(slot.session.master.as_raw_fd(), &ws);
            let clone = slot
                .session
                .master
                .try_clone()
                .map_err(std::io::Error::other)?;
            return Ok((id, clone));
        }
        let session = pty::spawn_login_shell(ws, &self.shell)?;
        let clone = session.master.try_clone().map_err(std::io::Error::other)?;
        let id = self.next_id;
        self.next_id += 1;
        self.slots.insert(
            id,
            Slot {
                session,
                attached: true,
            },
        );
        Ok((id, clone))
    }

    pub fn detach(&mut self, id: u64) {
        if let Some(slot) = self.slots.get_mut(&id) {
            slot.attached = false;
        }
    }

    pub fn drop_session(&mut self, id: u64) {
        if let Some(mut slot) = self.slots.remove(&id) {
            let _ = slot.session.child.kill();
            let _ = slot.session.child.wait();
        }
    }

    pub fn pts_name(&self, id: u64) -> Option<&str> {
        self.slots.get(&id).map(|s| s.session.pts_name.as_str())
    }

    fn reap(&mut self) {
        let dead: Vec<u64> = self
            .slots
            .iter_mut()
            .filter_map(|(id, slot)| match slot.session.child.try_wait() {
                Ok(Some(_)) => Some(*id),
                _ => None,
            })
            .collect();
        for id in dead {
            self.slots.remove(&id);
        }
    }
}

pub type SharedHub = Arc<Mutex<Hub>>;

pub fn new_shared(shell: String) -> SharedHub {
    Arc::new(Mutex::new(Hub::new(shell)))
}
