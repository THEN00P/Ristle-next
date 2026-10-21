//! An in-memory [`Fetch`]: files with ranges, chunked bodies, optional
//! gates and a request log. Stands in for `ristle-net`'s local test server
//! until workstream C lands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use ristle_media::{Fetch, FetchBody, FetchError, FetchRequest};
use tokio::sync::Semaphore;

/// Serves files from memory, with ranges, chunked bodies, optional gates
/// and a request log.
#[derive(Default)]
pub struct MemoryFetch {
    pub files: Mutex<HashMap<String, Bytes>>,
    log: Mutex<Vec<FetchRequest>>,
    /// Every chunk after a body's first waits for a permit here.
    gates: Mutex<HashMap<String, Arc<Semaphore>>>,
    chunk: usize,
}

impl MemoryFetch {
    pub fn new(chunk: usize) -> Self {
        Self {
            chunk,
            ..Self::default()
        }
    }

    pub fn put(&self, url: &str, data: impl Into<Bytes>) {
        self.files
            .lock()
            .unwrap()
            .insert(url.to_owned(), data.into());
    }

    pub fn gate(&self, url: &str) -> Arc<Semaphore> {
        let gate = Arc::new(Semaphore::new(0));
        self.gates
            .lock()
            .unwrap()
            .insert(url.to_owned(), gate.clone());
        gate
    }

    pub fn requests(&self) -> Vec<FetchRequest> {
        self.log.lock().unwrap().clone()
    }
}

struct MemoryBody {
    total: u64,
    data: Bytes,
    at: usize,
    chunk: usize,
    gate: Option<Arc<Semaphore>>,
}

#[async_trait]
impl FetchBody for MemoryBody {
    async fn chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        if self.at >= self.data.len() {
            return Ok(None);
        }
        if self.at > 0
            && let Some(gate) = &self.gate
        {
            gate.acquire()
                .await
                .map_err(|e| FetchError::Network(e.to_string()))?
                .forget();
        }
        let end = (self.at + self.chunk).min(self.data.len());
        let chunk = self.data.slice(self.at..end);
        self.at = end;
        Ok(Some(chunk))
    }

    fn total_length(&self) -> Option<u64> {
        Some(self.total)
    }
}

#[async_trait]
impl Fetch for MemoryFetch {
    async fn get(&self, request: FetchRequest) -> Result<Box<dyn FetchBody>, FetchError> {
        self.log.lock().unwrap().push(request.clone());
        let data = self
            .files
            .lock()
            .unwrap()
            .get(&request.url)
            .cloned()
            .ok_or(FetchError::Status { status: 404 })?;
        let total = data.len() as u64;
        let data = match request.range {
            Some(range) => {
                let start = usize::try_from(range.start).unwrap();
                if start > data.len() {
                    return Err(FetchError::Status { status: 416 });
                }
                let end = range
                    .length
                    .map_or(data.len(), |l| (start + l as usize).min(data.len()));
                data.slice(start..end)
            }
            None => data,
        };
        let gate = self.gates.lock().unwrap().get(&request.url).cloned();
        Ok(Box::new(MemoryBody {
            total,
            data,
            at: 0,
            chunk: self.chunk,
            gate,
        }))
    }
}
