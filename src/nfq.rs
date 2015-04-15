#![allow(non_camel_case_types)]

use libc::*;
use std::sync::{StaticMutex, MUTEX_INIT};
use std::error::Error;
use std::fmt;
use std::mem;
use std::ptr::null;

static NFQ_LOCK: StaticMutex = MUTEX_INIT;

#[derive(Debug)]
enum ErrorReason {
    OpenHandle,
    Bind,
    Unbind,
    CreateQueue,
    SetQueueMode,
    SetQueueMaxlen,
    Unknown
}

pub struct NFQError {
    reason: ErrorReason,
    description: String,
    cause: Option<Box<Error>>,
}

impl fmt::Debug for NFQError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let msg = format!("{:?}: {:?} (cause: {:?})",
                          self.reason, self.description, self.cause);
        formatter.write_str(msg.as_ref())
    }
}

impl fmt::Display for NFQError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let msg = format!("{:?} ({:?})", self.reason, self.description);
        formatter.write_str(msg.as_ref())
    }
}

impl Error for NFQError {
    fn description(&self) -> &str {
        self.description.as_ref()
    }
    fn cause(&self) -> Option<&Error> {
        self.cause.as_ref().map(|c| &**c)
    }
}

impl<E: Error + 'static> From<E> for NFQError {
    fn from(err: E) -> NFQError {
        NFQError {
            reason: ErrorReason::Unknown,
            description: err.description().to_string(),
            cause: Some(Box::new(err)),
        }
    }
}

fn error(reason: ErrorReason, msg: &str, res: Option<c_int>) -> NFQError {
    let errno = unsafe { nfq_errno };
    let desc = match res {
        Some(r) => format!("{} (errno: {}, res: {})", msg, errno, r),
        None => format!("{}, (errno: {})", msg, errno)
    };
    NFQError {
        reason: reason,
        description: desc,
        cause: None,
    }
}

#[link(name="netfilter_queue")]
extern {
    static mut nfq_errno: c_int;
}

// NFQ Handle

#[repr(C)]
struct nfq_handle;

#[repr(C)]
pub enum ProtoFamily {
    INET = 2,
    INET6 = 10,
}

#[link(name="netfilter_queue")]
extern {
    fn nfq_open() -> *mut nfq_handle;
    fn nfq_close(handle: *mut nfq_handle) -> c_int;

    fn nfq_bind_pf(handle: *mut nfq_handle, pf: uint16_t) -> c_int;
    fn nfq_unbind_pf(handle: *mut nfq_handle, pf: uint16_t) -> c_int;
}

pub struct NFQHandle {
    ptr: *mut nfq_handle,
}

impl Drop for NFQHandle {
    fn drop(&mut self) {
        let ret = unsafe { nfq_close(self.ptr) };
        if ret != 0 {
            panic!("Failed to close nfq handle");
        }
    }
}

impl NFQHandle {
    pub fn new() -> Result<NFQHandle, NFQError> {
        let _g = NFQ_LOCK.lock().unwrap();

        let ptr = unsafe { nfq_open() };
        if ptr.is_null() {
            Err(error(ErrorReason::OpenHandle, "Failed to allocate NFQHandle", None))
        } else {
            Ok(NFQHandle{ ptr: ptr })
        }
    }

    pub fn bind(&mut self, proto: ProtoFamily) -> Result<(), NFQError> {
        let _g = NFQ_LOCK.lock().unwrap();

        let res = unsafe { nfq_bind_pf(self.ptr, proto as uint16_t) };
        if res < 0 {
            Err(error(ErrorReason::Bind, "Failed to bind packet filter", Some(res)))
        } else {
            Ok(())
        }
    }

    pub fn unbind(&mut self, proto: ProtoFamily) -> Result<(), NFQError> {
        let _g = NFQ_LOCK.lock().unwrap();

        let res = unsafe { nfq_unbind_pf(self.ptr, proto as uint16_t) };
        if res < 0 {
            Err(error(ErrorReason::Bind, "Failed to unbind packet filter", Some(res)))
        } else {
            Ok(())
        }
    }

}

// NFQ Queue
#[repr(C)]
struct nfq_q_handle;

#[repr(C)]
struct nfgenmsg;

#[repr(C)]
struct nfq_data;

pub struct NFGenMsg {
    ptr: *mut nfgenmsg
}
pub struct NFQData {
    ptr: *mut nfq_data
}

#[repr(C)]
struct CallbackData<A> {
    ctx: *mut A,
    func: fn(ctx: &mut A, msg: &NFGenMsg, ad: &NFQData) -> c_int
}

enum NFQCopyMode {
    NONE = 0,
    META = 1,
    PACKET = 2
}

pub enum CopyMode {
    None,
    Metadata,
    Packet(u16)
}

#[link(name="netfilter_queue")]
extern {
    fn nfq_create_queue(handle: *mut nfq_handle,
                        num: uint16_t,
                        cb: extern "C" fn(h: *mut nfq_q_handle,
                                          nfmsg: *mut nfgenmsg,
                                          nfad: *mut nfq_data,
                                          data: *mut c_void) -> c_int,
                        data: *mut c_void) -> *mut nfq_q_handle;
    fn nfq_destroy_queue(handle: *mut nfq_q_handle) -> c_int;

    fn nfq_fd(handle: *mut nfq_handle) -> c_int;
    fn nfq_handle_packet(handle: *mut nfq_handle,
                         buf: *mut u8,
                         len: c_int) -> c_int;
    fn nfq_set_mode(handle: *mut nfq_q_handle,
                    mode: uint8_t,
                    range: uint32_t) -> c_int;
    fn nfq_set_queue_maxlen(handle: *mut nfq_q_handle,
                            queuelen: uint32_t) -> c_int;
    fn nfq_set_verdict(handle: *mut nfq_q_handle,
                       id: uint32_t,
                       verdict: uint32_t,
                       data_len: uint32_t,
                       buf: *const u8) -> c_int;
}

pub struct NFQQueue<A> {
    ptr: *mut nfq_q_handle,
    cb_data: CallbackData<A>,
    ctx: A
}

extern fn queue_callback<A>(h: *mut nfq_q_handle,
                            nfmsg: *mut nfgenmsg,
                            nfad: *mut nfq_data,
                            cdata: *mut c_void) -> c_int {
    let data: &CallbackData<A> = unsafe { mem::transmute(cdata) };
    let msg = NFGenMsg { ptr: nfmsg };
    let ad = NFQData { ptr: nfad };
    let mut ctx = match unsafe { data.ctx.as_mut() } {
        Some(c) => c,
        None => panic!("Could not deref ctx pointer")
    };
    (data.func)(ctx, &msg, &ad) as c_int
}

impl<A> NFQQueue<A> {
    pub fn new(h: &NFQHandle,
               num: u16,
               ctx: A,
               cb: fn(ctx: &mut A,
                      msg: &NFGenMsg,
                      ad: &NFQData) -> c_int
               ) -> Result<NFQQueue<A>, NFQError> {
        let _g = NFQ_LOCK.lock().unwrap();

        let fpointer: *const nfq_q_handle = null();
        let apointer: *const A = null();
        // So we initialise the queue obj as empty apart from the ctx
        // This is so we can take references to the mem inside
        let mut queue = NFQQueue {
            ctx: ctx,
            // Will be set after nfq_create_queue
            ptr: fpointer as *mut nfq_q_handle,
            cb_data: CallbackData{
                // Will be set to reference to queue.ctx
                ctx: apointer as *mut A,
                func: cb
            }
        };

        let ctxp: *mut A = &mut queue.ctx;
        queue.cb_data.ctx = ctxp;

        let ptr = unsafe { nfq_create_queue(h.ptr,
                                            num,
                                            queue_callback::<A>,
                                            mem::transmute(&queue.cb_data)) };
        if ptr.is_null() {
            Err(error(ErrorReason::CreateQueue, "Failed to create queue", None))
        } else {
            queue.ptr = ptr;
            Ok(queue)
        }
    }

    pub fn mode(&mut self, mode: CopyMode) -> Result<(), NFQError> {
        let cmode = match mode {
            CopyMode::None => NFQCopyMode::NONE,
            CopyMode::Metadata => NFQCopyMode::META,
            CopyMode::Packet(_) => NFQCopyMode::PACKET
        } as uint8_t;
        let range = match mode {
            CopyMode::Packet(r) => r,
            _ => 0
        } as uint16_t as uint32_t;

        let res = unsafe { nfq_set_mode(self.ptr, cmode, range) };
        if res != 0 {
            Err(error(ErrorReason::SetQueueMode, "Failed to set queue mode", Some(res)))
        } else {
            Ok(())
        }
    }

    pub fn queue_maxlen(&mut self, len: u32) -> Result<(), NFQError> {
        let res = unsafe { nfq_set_queue_maxlen(self.ptr, len as uint32_t) };
        if res != 0 {
            Err(error(ErrorReason::SetQueueMaxlen, "Failed to set queue maxlen", Some(res)))
        } else {
            Ok(())
        }
    }
}

impl<A> Drop for NFQQueue<A> {
    fn drop(&mut self) {
        let ret = unsafe { nfq_destroy_queue(self.ptr) };
        if ret != 0 {
            panic!("Failed to destroy nfq queue");
        }
    }
}
