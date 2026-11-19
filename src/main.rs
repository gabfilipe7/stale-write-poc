#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::ffi::{CStr, c_void};
use std::mem::zeroed;
use std::ptr::null_mut;

const DEVICE: &str = "rxe0";
const PORT: u8 = 1;
const GID_INDEX: u8 = 1;
const SLOT_SIZE: usize = 8;

fn die(what: &str) -> ! {
    eprintln!("{what}: {}", std::io::Error::last_os_error());
    std::process::exit(1);
}

unsafe fn modify(qp: *mut ibv_qp, attr: &mut ibv_qp_attr, mask: u32, what: &str) {
    if unsafe { ibv_modify_qp(qp, attr, mask as i32) } != 0 {
        die(what);
    }
}

unsafe fn connect(qp: *mut ibv_qp, remote_qpn: u32, remote_gid: ibv_gid) {
    unsafe {
        let mut a: ibv_qp_attr = zeroed();
        a.qp_state = IBV_QPS_INIT;
        a.port_num = PORT;
        a.pkey_index = 0;
        a.qp_access_flags = IBV_ACCESS_REMOTE_WRITE;
        modify(
            qp,
            &mut a,
            IBV_QP_STATE | IBV_QP_PORT | IBV_QP_PKEY_INDEX | IBV_QP_ACCESS_FLAGS,
            "modify_qp INIT",
        );

        let mut a: ibv_qp_attr = zeroed();
        a.qp_state = IBV_QPS_RTR;
        a.path_mtu = IBV_MTU_1024;
        a.dest_qp_num = remote_qpn;
        a.rq_psn = 0;
        a.max_dest_rd_atomic = 1;
        a.min_rnr_timer = 12;
        a.ah_attr.is_global = 1;
        a.ah_attr.port_num = PORT;
        a.ah_attr.grh.dgid = remote_gid;
        a.ah_attr.grh.sgid_index = GID_INDEX;
        a.ah_attr.grh.hop_limit = 1;
        modify(
            qp,
            &mut a,
            IBV_QP_STATE
                | IBV_QP_AV
                | IBV_QP_PATH_MTU
                | IBV_QP_DEST_QPN
                | IBV_QP_RQ_PSN
                | IBV_QP_MAX_DEST_RD_ATOMIC
                | IBV_QP_MIN_RNR_TIMER,
            "modify_qp RTR",
        );

        let mut a: ibv_qp_attr = zeroed();
        a.qp_state = IBV_QPS_RTS;
        a.sq_psn = 0;
        a.timeout = 14;
        a.retry_cnt = 7;
        a.rnr_retry = 7;
        a.max_rd_atomic = 1;
        modify(
            qp,
            &mut a,
            IBV_QP_STATE
                | IBV_QP_SQ_PSN
                | IBV_QP_TIMEOUT
                | IBV_QP_RETRY_CNT
                | IBV_QP_RNR_RETRY
                | IBV_QP_MAX_QP_RD_ATOMIC,
            "modify_qp RTS",
        );
    }
}

unsafe fn open_device(name: &str) -> *mut ibv_context {
    unsafe {
        let mut n = 0;
        let list = ibv_get_device_list(&mut n);
        if list.is_null() {
            die("ibv_get_device_list");
        }
        let mut ctx = null_mut();
        for i in 0..n as usize {
            let dev = *list.add(i);
            if CStr::from_ptr(ibv_get_device_name(dev)).to_str() == Ok(name) {
                ctx = ibv_open_device(dev);
                break;
            }
        }
        ibv_free_device_list(list);
        ctx
    }
}

fn main() {
    unsafe {
        let ctx = open_device(DEVICE);
        if ctx.is_null() {
            die("open rxe0 (run ./setup_rxe.sh first)");
        }

        let mut gid: ibv_gid = zeroed();
        if ibv_query_gid(ctx, PORT, GID_INDEX as i32, &mut gid) != 0 {
            die("ibv_query_gid");
        }

        let pd = ibv_alloc_pd(ctx);
        if pd.is_null() {
            die("ibv_alloc_pd");
        }

        let cq = ibv_create_cq(ctx, 16, null_mut(), null_mut(), 0);
        if cq.is_null() {
            die("ibv_create_cq");
        }

        let mut init: ibv_qp_init_attr = zeroed();
        init.qp_type = IBV_QPT_RC;
        init.send_cq = cq;
        init.recv_cq = cq;
        init.cap.max_send_wr = 4;
        init.cap.max_recv_wr = 4;
        init.cap.max_send_sge = 1;
        init.cap.max_recv_sge = 1;
        let owner_qp = ibv_create_qp(pd, &mut init);
        let client_qp = ibv_create_qp(pd, &mut init);
        if owner_qp.is_null() || client_qp.is_null() {
            die("ibv_create_qp");
        }

        connect(owner_qp, (*client_qp).qp_num, gid);
        connect(client_qp, (*owner_qp).qp_num, gid);

        let mut slot = vec![0u8; SLOT_SIZE];
        let slot_ptr = slot.as_mut_ptr();
        let access = IBV_ACCESS_LOCAL_WRITE | IBV_ACCESS_REMOTE_WRITE;
        let slot_mr = ibv_reg_mr(pd, slot_ptr as *mut c_void, SLOT_SIZE, access as i32);
        if slot_mr.is_null() {
            die("ibv_reg_mr slot");
        }

        let mut alice_buf = [0xA1u8; SLOT_SIZE];
        let alice_mr = ibv_reg_mr(pd, alice_buf.as_mut_ptr() as *mut c_void, SLOT_SIZE, 0);
        if alice_mr.is_null() {
            die("ibv_reg_mr alice");
        }

        let read_slot =
            || -> Vec<u8> { (0..SLOT_SIZE).map(|i| slot_ptr.add(i).read_volatile()).collect() };

        let addr = slot_ptr as u64;
        let rkey = (*slot_mr).rkey;
        println!("[1] owner -> alice: key K (rkey={rkey:#x})");
        println!("[2] alice delayed");

        for i in 0..SLOT_SIZE {
            slot_ptr.add(i).write_volatile(0xB0);
        }
        println!("[3] slot reassigned to bob: {:02x?}", read_slot());

        let mut sge: ibv_sge = zeroed();
        sge.addr = alice_buf.as_ptr() as u64;
        sge.length = SLOT_SIZE as u32;
        sge.lkey = (*alice_mr).lkey;

        let mut wr: ibv_send_wr = zeroed();
        wr.opcode = IBV_WR_RDMA_WRITE;
        wr.sg_list = &mut sge;
        wr.num_sge = 1;
        wr.send_flags = IBV_SEND_SIGNALED;
        wr.wr.rdma.remote_addr = addr;
        wr.wr.rdma.rkey = rkey;

        let mut bad_wr = null_mut();
        if ibv_post_send(client_qp, &mut wr, &mut bad_wr) != 0 {
            die("ibv_post_send");
        }

        let mut wc: ibv_wc = zeroed();
        loop {
            match ibv_poll_cq(cq, 1, &mut wc) {
                0 => continue,
                1 => break,
                _ => die("ibv_poll_cq"),
            }
        }
        let status = CStr::from_ptr(ibv_wc_status_str(wc.status)).to_string_lossy();
        println!("[4] alice late write with K: {status}");

        let after = read_slot();
        println!("[5] slot now: {after:02x?}");

        if wc.status == IBV_WC_SUCCESS && after.iter().all(|&b| b == 0xA1) {
            println!("gap reproduced: bob's data overwritten via stale key, no error reported");
        } else {
            println!("gap not reproduced");
        }

        if ibv_dereg_mr(alice_mr) != 0 || ibv_dereg_mr(slot_mr) != 0 {
            die("ibv_dereg_mr");
        }
        if ibv_destroy_qp(client_qp) != 0 || ibv_destroy_qp(owner_qp) != 0 {
            die("ibv_destroy_qp");
        }
        if ibv_destroy_cq(cq) != 0 {
            die("ibv_destroy_cq");
        }
        if ibv_dealloc_pd(pd) != 0 {
            die("ibv_dealloc_pd");
        }
        if ibv_close_device(ctx) != 0 {
            die("ibv_close_device");
        }
    }
}
