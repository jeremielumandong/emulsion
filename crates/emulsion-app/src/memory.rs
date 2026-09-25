//! Keep large image buffers independently releasable on glibc.

/// Call at process startup, before creating any application worker threads.
pub(crate) fn configure() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // JPEG decode buffers can raise glibc's adaptive mmap threshold above
        // our 512 KiB pixel tiles. The tiles then enter thread-local heaps,
        // which can retain hundreds of MiB even after the document is dropped.
        // Pin the initial 128 KiB threshold so large allocations can be unmapped
        // on free. Small UI allocations keep the normal heap allocation path.
        // https://sourceware.org/glibc/manual/latest/html_node/Malloc-Tunable-Parameters.html
        // SAFETY: this configures glibc before application threads are started;
        // the parameter and threshold are valid mallopt inputs.
        unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, 128 * 1024) != 0 }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        true
    }
}
