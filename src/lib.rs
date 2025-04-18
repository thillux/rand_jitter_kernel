//! This crate provides an interface to the `jitterentropy_rng` inside the Linux kernel

use rand_core::TryRngCore;

const MAX_RETURN_CHUNK_SIZE: usize = 128;

/// data structure holding state of the rng
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RandJitterKernel {
    rng_fd: libc::c_int,
}

impl RandJitterKernel {
    /// constructs new RNG instance
    ///
    /// # Errors
    /// For all used errors, a different string reason is returned inside `std::io::Error::other(..)`.
    pub fn new() -> Result<Self, std::io::Error> {
        /*
         * We need to open a socket to declare the algorithm to be used first (fam_fd).
         * In a next step, we accept on this socket to get a specific instance (rng_fd).
         * After getting the instance, we can close fam_fd.
         */

        // AF_ALG with jitterentropy_rng is currently only implemented inside the Linux kernel
        #[cfg(not(target_os = "linux"))]
        compile_error!("Only Linux is supported");

        // close this on every (early) return!
        let fam_fd = unsafe { libc::socket(libc::AF_ALG, libc::SOCK_SEQPACKET, 0) };
        if fam_fd < 0 {
            return Err(std::io::Error::other(
                "unable to create AF_ALG socket for jitterentropy_rng",
            ));
        }

        let mut sock_addr: libc::sockaddr_alg = unsafe { std::mem::zeroed() };
        sock_addr.salg_family = u16::try_from(libc::AF_ALG)
            .map_err(|_| std::io::Error::other("unable to convert socket algorithm family"))?;
        let rng_type = "rng";
        let rng_name = "jitterentropy_rng";

        sock_addr.salg_type[..rng_type.len()].copy_from_slice(rng_type.to_string().as_bytes());
        sock_addr.salg_name[..rng_name.len()].copy_from_slice(rng_name.to_string().as_bytes());

        let bind_ret = unsafe {
            libc::bind(
                fam_fd,
                std::ptr::addr_of!(sock_addr).cast::<libc::sockaddr>(),
                u32::try_from(std::mem::size_of_val(&sock_addr))
                    .map_err(|_| std::io::Error::other("unable to convert size of sock_addr"))?,
            )
        };
        if bind_ret != 0 {
            unsafe {
                libc::close(fam_fd);
            }
            return Err(std::io::Error::other("unable to bind AF_ALG socket"));
        }

        let rng_fd = unsafe { libc::accept(fam_fd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if rng_fd < 0 {
            unsafe {
                libc::close(fam_fd);
            }
            return Err(std::io::Error::other("unable to get rng_fd from kernel"));
        }

        // as we now got the specific rng_fd instance, we can close the fd announcing the type of algorithm
        // we are interested in
        unsafe { libc::close(fam_fd) };

        Ok(RandJitterKernel { rng_fd })
    }

    fn try_fill_bytes_max_chunk_size(&mut self, dst: &mut [u8]) -> Result<(), std::io::Error> {
        if dst.len() > MAX_RETURN_CHUNK_SIZE {
            return Err(std::io::Error::other(format!(
                "Cannot return more than {} byte in a single call. Requested: {} byte",
                MAX_RETURN_CHUNK_SIZE,
                dst.len()
            )));
        }

        if self.rng_fd < 0 {
            return Err(std::io::Error::other(format!(
                "Cannot get entropy from jitterentropy_rng in kernel with invalid fd {}",
                self.rng_fd
            )));
        }

        let size = unsafe {
            libc::read(
                self.rng_fd,
                dst.as_mut_ptr().cast::<libc::c_void>(),
                dst.len(),
            )
        };

        if size >= 0
            && usize::try_from(size)
                .map_err(|_| std::io::Error::other("unable to convert returned size to usize"))?
                == dst.len()
        {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "Cannot get entropy from jitterentropy_rng in kernel",
            ))
        }
    }
}

impl Default for RandJitterKernel {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

impl Drop for RandJitterKernel {
    fn drop(&mut self) {
        assert!(self.rng_fd >= 0, "rng_fd already closed or never opened?");
        unsafe {
            libc::close(self.rng_fd);
        }
        self.rng_fd = -1;
    }
}

impl TryRngCore for RandJitterKernel {
    type Error = std::io::Error;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(u32::try_from(self.try_next_u64()? & 0xFF_FF_FF_FF).unwrap())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut bytes: [u8; 8] = [0; 8];
        self.try_fill_bytes(&mut bytes)?;

        Ok(u64::from_ne_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        let mut idx = 0;
        while idx < dst.len() {
            let chunk_size = if idx + MAX_RETURN_CHUNK_SIZE > dst.len() {
                dst.len() - idx
            } else {
                MAX_RETURN_CHUNK_SIZE
            };
            self.try_fill_bytes_max_chunk_size(&mut dst[idx..idx + chunk_size])?;
            idx += chunk_size;
        }
        assert_eq!(idx, dst.len());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::RandJitterKernel;
    use rand_core::TryRngCore;

    #[test]
    fn test_u32() {
        let mut rng = RandJitterKernel::new().unwrap();
        for _ in 0..1000 {
            let u = rng.try_next_u32();
            assert!(u.is_ok());
        }
    }

    #[test]
    fn test_u64() {
        let mut rng = RandJitterKernel::new().unwrap();
        for _ in 0..1000 {
            let u = rng.try_next_u64();
            assert!(u.is_ok());
        }
    }

    #[test]
    fn test_speed() {
        use std::time::Instant;
        let start = Instant::now();
        let mut num_bytes = 0usize;
        let mut rng = RandJitterKernel::new().unwrap();

        loop {
            let mut b = [0u8; 32];
            rng.try_fill_bytes(&mut b).unwrap();

            let now = Instant::now();

            num_bytes += b.len();

            if (now - start).as_secs() > 2 {
                let datarate = f64::from(u32::try_from(num_bytes).unwrap())
                    / (now - start).as_secs_f64()
                    / 1024.0;
                println!("datarate: {datarate} KiB/s");
                break;
            }
        }
    }

    #[test]
    fn test_bytes() {
        let mut rng = RandJitterKernel::new().unwrap();

        for buffer_size in 0..=256 {
            let mut buffer = vec![0u8; buffer_size];
            assert!(rng.try_fill_bytes(&mut buffer).is_ok());
            println!("{buffer_size}: {buffer:#04X?}");
        }
    }

    #[test]
    fn test_large_bytes_but_ok() {
        let mut rng = RandJitterKernel::new().unwrap();
        let mut buffer = [0u8; 128];
        assert!(rng.try_fill_bytes_max_chunk_size(&mut buffer).is_ok());
    }

    #[test]
    fn test_too_large_bytes() {
        let mut rng = RandJitterKernel::new().unwrap();
        let mut buffer = [0u8; 129];
        assert!(rng.try_fill_bytes_max_chunk_size(&mut buffer).is_err());
    }

    #[test]
    fn test_multi_instantiation() {
        for _ in 0..256 {
            let mut rng = RandJitterKernel::new().unwrap();
            let u = rng.try_next_u32().unwrap();
            println!("Got {u}");
        }
    }

    #[test]
    fn test_multi_threading() {
        let mut threads = vec![];
        let mut rng = RandJitterKernel::new().unwrap();
        let _ = rng.try_next_u64().unwrap();

        println!("Got bytes (single threaded)!");

        for _ in 0..6 {
            threads.push(std::thread::spawn(move || {
                for _ in 0..1024 {
                    let mut rng = RandJitterKernel::new().unwrap();
                    let _ = rng.try_next_u64().unwrap();
                }
            }));
        }

        for t in threads {
            let _ = t.join();
        }
    }
}
