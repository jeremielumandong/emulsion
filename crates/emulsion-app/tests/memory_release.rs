//! A fresh process is required: mallopt must run before worker threads start.
#[path = "../src/memory.rs"]
mod memory;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn main() {
    use std::sync::{Arc, Barrier, mpsc};

    assert!(memory::configure());
    fn rss() -> usize {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            * 1024
    }

    const MIB: usize = 1024 * 1024;
    let baseline = rss();
    // Decoding and dropping a large contiguous buffer used to raise glibc's
    // adaptive threshold above our tiles, even before the first document.
    let decoded = vec![123u8; 16 * MIB];
    std::hint::black_box(&decoded);
    drop(decoded);

    let barrier = Arc::new(Barrier::new(9));
    let (send, receive) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let send = send.clone();
            let barrier = barrier.clone();
            scope.spawn(move || {
                let tiles: Vec<Arc<[u8]>> = (0..64)
                    .map(|_| Arc::from(vec![123u8; 512 * 1024]))
                    .collect();
                send.send(tiles).unwrap();
                // Real image workers survive document closure. Exiting them
                // here would hide memory retained by their allocator caches.
                barrier.wait();
            });
        }
        let tiles: Vec<_> = (0..8).map(|_| receive.recv().unwrap()).collect();
        let loaded = rss();
        std::hint::black_box(&tiles);
        drop(tiles);
        let closed = rss();
        // Always release the workers before assertions, including failures.
        barrier.wait();
        println!(
            "RSS MiB: baseline={}, loaded={}, closed={}",
            baseline / MIB,
            loaded / MIB,
            closed / MIB
        );
        assert!(
            loaded > baseline + 240 * MIB,
            "tile buffers were not resident"
        );
        assert!(
            closed < baseline + 32 * MIB,
            "closed image buffers stayed resident"
        );
    });
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn main() {
    assert!(memory::configure());
}
