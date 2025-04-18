use rand_core::TryRngCore;

#[derive(Debug, PartialEq, PartialOrd)]
pub struct RandJitterKernel {
    fam_fd: libc::c_int,
    rng_fd: libc::c_int,
}

impl RandJitterKernel {
    #[allow(dead_code)]
    fn new() -> Result<Self, std::io::Error> {
        let fam_fd = unsafe { libc::socket(libc::AF_ALG, libc::SOCK_SEQPACKET, 0) };
        if fam_fd <= 0 {
            return Err(std::io::Error::other(
                "unable to create AF_ALG socket for jitterentropy_rng",
            ));
        }

        let mut sock_addr: libc::sockaddr_alg = unsafe { std::mem::zeroed() };
        sock_addr.salg_family = u16::try_from(libc::AF_ALG).unwrap();
        let rng_type = "rng";
        let rng_name = "jitterentropy_rng";

        sock_addr.salg_type[..rng_type.len()].copy_from_slice(rng_type.to_string().as_bytes());
        sock_addr.salg_name[..rng_name.len()].copy_from_slice(rng_name.to_string().as_bytes());

        let bind_ret = unsafe {
            libc::bind(
                fam_fd,
                std::ptr::addr_of!(sock_addr).cast::<libc::sockaddr>(),
                u32::try_from(std::mem::size_of_val(&sock_addr)).unwrap(),
            )
        };
        if bind_ret != 0 {
            return Err(std::io::Error::other("unable to bind AF_ALG socket"));
        }

        let rng_fd = unsafe { libc::accept(fam_fd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if rng_fd <= 0 {
            return Err(std::io::Error::other("unable to get rng_fd from kernel"));
        }

        Ok(RandJitterKernel { fam_fd, rng_fd })
    }
}

impl Drop for RandJitterKernel {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.rng_fd);
            libc::close(self.fam_fd);
        }
        self.rng_fd = -1;
        self.fam_fd = -1;
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
        let size = unsafe {
            libc::read(
                self.rng_fd,
                dst.as_mut_ptr().cast::<libc::c_void>(),
                dst.len(),
            )
        };

        if size >= 0 && usize::try_from(size).unwrap() == dst.len() {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "Cannot get entropy from jitterentropy_rng in kernel",
            ))
        }
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

        for buffer_size in 0..=128 {
            let mut buffer = vec![0u8; buffer_size];
            assert!(rng.try_fill_bytes(&mut buffer).is_ok());
        }

        for buffer_size in 129..256 {
            let mut buffer = vec![0u8; buffer_size];
            assert!(rng.try_fill_bytes(&mut buffer).is_err());
        }
    }

    #[test]
    fn test_large_bytes_but_ok() {
        let mut rng = RandJitterKernel::new().unwrap();
        let mut buffer = [0u8; 128];
        assert!(rng.try_fill_bytes(&mut buffer).is_ok());
    }

    #[test]
    fn test_too_large_bytes() {
        let mut rng = RandJitterKernel::new().unwrap();
        let mut buffer = [0u8; 129];
        assert!(rng.try_fill_bytes(&mut buffer).is_err());
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
