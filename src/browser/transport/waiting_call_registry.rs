use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc;

use anyhow::Result;
use log::trace;

use crate::types::{CallId, Response};

use super::ConnectionClosed;

trait IdentifiableResponse {
    fn call_id(&self) -> CallId;
}

#[derive(Debug)]
pub struct WaitingCallRegistry {
    calls: Mutex<HashMap<CallId, mpsc::Sender<Result<Response>>>>,
}

impl IdentifiableResponse for Response {
    fn call_id(&self) -> CallId {
        self.call_id
    }
}

impl Default for WaitingCallRegistry {
    fn default() -> Self {
        let calls = Mutex::new(HashMap::new());

        Self { calls }
    }
}

impl WaitingCallRegistry {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn resolve_call(&self, response: Response) {
        trace!("Resolving call");
        let call_id = response.call_id();
        let waiting_call_tx: Option<mpsc::Sender<Result<Response>>> = {
            let mut waiting_calls = self.calls.lock().unwrap();
            waiting_calls.remove(&call_id)
        };

        // A call is missing from the registry when its caller already gave up on it
        // (e.g. it timed out), which is not an error: there's simply nobody left to
        // hand the response to.
        let Some(waiting_call_tx) = waiting_call_tx else {
            trace!("Got a response for an unregistered call: {call_id:?}");
            return;
        };

        if let Err(e) = waiting_call_tx.send(Ok(response)) {
            trace!("Waiting call {call_id:?} stopped listening for its response: {e:?}");
        }
    }

    pub fn register_call(&self, call_id: CallId) -> mpsc::Receiver<Result<Response>> {
        let (tx, rx) = mpsc::channel::<Result<Response>>();
        let mut calls = self.calls.lock().unwrap();
        calls.insert(call_id, tx);
        trace!("registered {call_id:?}");
        rx
    }

    pub fn unregister_call(&self, call_id: CallId) {
        trace!("Deregistering call");
        let mut calls = self.calls.lock().unwrap();
        // The call may have been resolved by the transport loop in the meantime.
        calls.remove(&call_id);
    }

    // TODO: make it so we can pass in whatever error we want here
    // to make it less dependent on browser::transport
    pub fn cancel_outstanding_method_calls(&self) {
        trace!("Cancelling outstanding method calls");
        let mut calls = self.calls.lock().unwrap();
        for (call_id, sender) in calls.drain() {
            trace!("Telling waiting method call {call_id:?} that the connection closed");
            if let Err(e) = sender.send(Err(ConnectionClosed {}.into())) {
                trace!(
                    "Couldn't send ConnectionClosed to waiting method call: {call_id:?} because {e:?}",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn register_and_receive_calls() {
        env_logger::try_init().unwrap_or(());

        let waiting_calls = WaitingCallRegistry::new();

        let call_rx = waiting_calls.register_call(431);
        let resp = Response {
            call_id: 431,
            result: Some(json! {true}),
            error: None,
        };
        let resp_clone = resp.clone();

        let call_rx2 = waiting_calls.register_call(123);
        let resp2 = Response {
            call_id: 123,
            result: Some(json! {false}),
            error: None,
        };
        let cloned_resp = resp2.clone();

        waiting_calls.resolve_call(resp);
        waiting_calls.resolve_call(resp2);

        // note how they're in reverse order to that in which they were called!
        assert_eq!(cloned_resp, call_rx2.recv().unwrap().unwrap());
        assert_eq!(resp_clone, call_rx.recv().unwrap().unwrap());
    }
}
