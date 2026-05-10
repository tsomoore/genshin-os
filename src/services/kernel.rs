// Kernel — central message dispatcher
//
// Only component subscribed to the message bus. Routes messages
// to service-specific channels. Services never touch the bus directly
// for receiving—they only use it for sending.

use std::sync::Arc;
use crossbeam_channel::{unbounded, Sender, Receiver};
use crate::messaging::{KernelMsg, MessageBus};
use crate::messaging::bus::Envelope;

pub struct Kernel {
    bus: Arc<dyn MessageBus>,
    receiver: Receiver<Envelope>,
    process_tx: Sender<Envelope>,
    memory_tx: Sender<Envelope>,
    file_tx: Sender<Envelope>,
}

impl Kernel {
    pub fn new(bus: Arc<dyn MessageBus>) -> (Self, Receiver<Envelope>, Receiver<Envelope>, Receiver<Envelope>) {
        let receiver = bus.subscribe();
        let (ptx, prx) = unbounded();
        let (mtx, mrx) = unbounded();
        let (ftx, frx) = unbounded();
        (Self { bus, receiver, process_tx: ptx, memory_tx: mtx, file_tx: ftx }, prx, mrx, frx)
    }

    pub fn run(&self) {
        println!("Kernel starting...");
        loop {
            match self.receiver.recv() {
                Ok(envelope) => { self.route(envelope); }
                Err(_) => { eprintln!("Kernel: bus disconnected"); break; }
            }
        }
    }

    fn route(&self, envelope: Envelope) {
        let msg = &envelope.message;
        match msg {
            KernelMsg::Process(_) | KernelMsg::Syscall(_) | KernelMsg::Interrupt(_) => {
                let _ = self.process_tx.send(envelope);
            }
            KernelMsg::Memory(_) => {
                let _ = self.memory_tx.send(envelope);
            }
            KernelMsg::File(_) => {
                let _ = self.file_tx.send(envelope);
            }
            KernelMsg::Device(_) => {}
    }
}
