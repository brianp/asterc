use std::os::fd::RawFd;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // ReadWrite will be used when I/O primitives are added
pub(crate) enum Interest {
    Read,
    Write,
    ReadWrite,
}

/// Token maps back to a task/green-thread identity.
#[derive(Clone, Copy)]
pub(crate) struct Token(pub usize);

#[allow(dead_code)] // readable/writable used by future I/O primitives
pub(crate) struct Event {
    pub token: Token,
    pub readable: bool,
    pub writable: bool,
}

pub(crate) trait Poller: Send {
    fn register(&mut self, fd: RawFd, interest: Interest, token: Token);
    fn deregister(&mut self, fd: RawFd);
    fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> usize;
}

// ---------------------------------------------------------------------------
// macOS: kqueue
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod kqueue_impl {
    use super::*;

    pub(crate) struct KqueuePoller {
        kq: RawFd,
    }

    impl KqueuePoller {
        pub(crate) fn new() -> Self {
            let kq = unsafe { libc::kqueue() };
            assert!(kq >= 0, "kqueue() failed");
            Self { kq }
        }
    }

    impl Drop for KqueuePoller {
        fn drop(&mut self) {
            unsafe { libc::close(self.kq) };
        }
    }

    impl Poller for KqueuePoller {
        fn register(&mut self, fd: RawFd, interest: Interest, token: Token) {
            let mut changes = Vec::with_capacity(2);

            if matches!(interest, Interest::Read | Interest::ReadWrite) {
                let mut ev: libc::kevent = unsafe { std::mem::zeroed() };
                ev.ident = fd as usize;
                ev.filter = libc::EVFILT_READ;
                ev.flags = libc::EV_ADD | libc::EV_ONESHOT;
                ev.udata = token.0 as *mut _;
                changes.push(ev);
            }
            if matches!(interest, Interest::Write | Interest::ReadWrite) {
                let mut ev: libc::kevent = unsafe { std::mem::zeroed() };
                ev.ident = fd as usize;
                ev.filter = libc::EVFILT_WRITE;
                ev.flags = libc::EV_ADD | libc::EV_ONESHOT;
                ev.udata = token.0 as *mut _;
                changes.push(ev);
            }

            let rc = unsafe {
                libc::kevent(
                    self.kq,
                    changes.as_ptr(),
                    changes.len() as i32,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            assert!(rc >= 0, "kevent register failed");
        }

        fn deregister(&mut self, fd: RawFd) {
            // EV_DELETE for both read and write filters (ignore errors if not registered)
            for filter in [libc::EVFILT_READ, libc::EVFILT_WRITE] {
                let mut ev: libc::kevent = unsafe { std::mem::zeroed() };
                ev.ident = fd as usize;
                ev.filter = filter;
                ev.flags = libc::EV_DELETE;
                unsafe {
                    libc::kevent(self.kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null());
                };
            }
        }

        fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> usize {
            let mut kevents = [unsafe { std::mem::zeroed::<libc::kevent>() }; 64];

            let ts = timeout.map(|d| libc::timespec {
                tv_sec: d.as_secs() as libc::time_t,
                tv_nsec: d.subsec_nanos() as libc::c_long,
            });
            let ts_ptr = match ts.as_ref() {
                Some(t) => t as *const _,
                None => std::ptr::null(),
            };

            let n = unsafe {
                libc::kevent(
                    self.kq,
                    std::ptr::null(),
                    0,
                    kevents.as_mut_ptr(),
                    kevents.len() as i32,
                    ts_ptr,
                )
            };

            if n < 0 {
                return 0;
            }

            let count = n as usize;
            for kev in &kevents[..count] {
                events.push(Event {
                    token: Token(kev.udata as usize),
                    readable: kev.filter == libc::EVFILT_READ,
                    writable: kev.filter == libc::EVFILT_WRITE,
                });
            }
            count
        }
    }
}

// ---------------------------------------------------------------------------
// Linux: epoll
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod epoll_impl {
    use super::*;

    pub(crate) struct EpollPoller {
        epfd: RawFd,
    }

    impl EpollPoller {
        pub(crate) fn new() -> Self {
            let epfd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
            assert!(epfd >= 0, "epoll_create1() failed");
            Self { epfd }
        }
    }

    impl Drop for EpollPoller {
        fn drop(&mut self) {
            unsafe { libc::close(self.epfd) };
        }
    }

    impl Poller for EpollPoller {
        fn register(&mut self, fd: RawFd, interest: Interest, token: Token) {
            let mut events = 0u32;
            if matches!(interest, Interest::Read | Interest::ReadWrite) {
                events |= libc::EPOLLIN as u32;
            }
            if matches!(interest, Interest::Write | Interest::ReadWrite) {
                events |= libc::EPOLLOUT as u32;
            }
            events |= libc::EPOLLONESHOT as u32;

            let mut ev = libc::epoll_event {
                events,
                u64: token.0 as u64,
            };
            let rc = unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_ADD, fd, &mut ev) };
            if rc < 0 {
                // Might already be registered — try MOD
                unsafe { libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_MOD, fd, &mut ev) };
            }
        }

        fn deregister(&mut self, fd: RawFd) {
            unsafe {
                libc::epoll_ctl(self.epfd, libc::EPOLL_CTL_DEL, fd, std::ptr::null_mut());
            };
        }

        fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> usize {
            let mut epevents = [unsafe { std::mem::zeroed::<libc::epoll_event>() }; 64];
            let timeout_ms = match timeout {
                Some(d) => d.as_millis() as i32,
                None => -1,
            };

            let n = unsafe {
                libc::epoll_wait(
                    self.epfd,
                    epevents.as_mut_ptr(),
                    epevents.len() as i32,
                    timeout_ms,
                )
            };

            if n < 0 {
                return 0;
            }

            let count = n as usize;
            for ep in epevents.iter().take(count) {
                events.push(Event {
                    token: Token(ep.u64 as usize),
                    readable: (ep.events & libc::EPOLLIN as u32) != 0,
                    writable: (ep.events & libc::EPOLLOUT as u32) != 0,
                });
            }
            count
        }
    }
}

/// Create a platform-appropriate poller.
pub(crate) fn create_poller() -> Box<dyn Poller> {
    #[cfg(target_os = "macos")]
    {
        Box::new(kqueue_impl::KqueuePoller::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(epoll_impl::EpollPoller::new())
    }
}
