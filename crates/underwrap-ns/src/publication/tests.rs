use super::*;
use std::io::{Read, Write};
use std::thread;
use underwrap_protocol::libsecret_publication::{decode_response, encode_request_header};

fn publish(stream: &mut UnixStream, generation: u64, body: &[u8]) -> ResponseKind {
    stream
        .write_all(&encode_request_header(generation, body.len()).unwrap())
        .unwrap();
    stream.write_all(body).unwrap();
    let mut response = [0; RESPONSE_LEN];
    stream.read_exact(&mut response).unwrap();
    let (kind, actual) = decode_response(&response).unwrap();
    assert_eq!(actual, generation);
    kind
}

#[test]
fn publication_reserved_fd_collisions_are_relocated_in_isolated_children() {
    for reserved in RESERVED_HANDOFF_FDS {
        // SAFETY: fork isolates the descriptor-table mutation from concurrent
        // Rust tests; the child performs only bounded fd syscalls before _exit.
        let child = unsafe { libc::fork() };
        assert!(child >= 0, "fork failed");
        if child == 0 {
            let dev_null = std::ffi::CString::new("/dev/null").unwrap();
            // SAFETY: the C string is valid and open returns a fresh descriptor.
            let opened = unsafe { libc::open(dev_null.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            if opened < 0 {
                unsafe { libc::_exit(10) }
            }
            // SAFETY: duplicate the live opened descriptor above the synthetic
            // collision range, then close only the original ownership.
            let source = unsafe { libc::fcntl(opened, libc::F_DUPFD_CLOEXEC, 30) };
            unsafe { libc::close(opened) };
            if source < 0 {
                unsafe { libc::_exit(11) }
            }
            for fd in 3..=21 {
                // SAFETY: source stays live above this range and dup2 creates
                // the deliberate child-local occupancy at exactly fd.
                if unsafe { libc::dup2(source, fd) } != fd {
                    unsafe { libc::_exit(12) }
                }
            }
            // SAFETY: reserved is one synthetic duplicate owned by this child;
            // closing it creates the exact collision under test.
            unsafe { libc::close(reserved) };
            let code = match PublicationPair::new() {
                Ok(pair)
                    if pair.host.stream.as_raw_fd() >= FIRST_PRIVATE_FD
                        && pair.publisher.as_raw_fd() >= FIRST_PRIVATE_FD
                        && require_cloexec(&pair.host.stream).is_ok()
                        && require_cloexec(&pair.publisher).is_ok() =>
                {
                    0
                }
                _ => 13,
            };
            // SAFETY: the isolated child reports only its bounded assertion
            // result; process exit closes all synthetic descriptors.
            unsafe { libc::_exit(code) }
        }

        let mut status = 0;
        loop {
            // SAFETY: status is writable and child is the direct child above.
            let result = unsafe { libc::waitpid(child, &mut status, 0) };
            if result == child {
                break;
            }
            if result < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            panic!("waitpid failed for reserved fd {reserved}");
        }
        assert!(
            libc::WIFEXITED(status),
            "reserved fd {reserved}: status {status}"
        );
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "reserved fd {reserved}: status {status}"
        );
    }
}

#[test]
fn publication_socketpair_is_cloexec_and_initial_ack_selects_generation_zero() {
    let pair = PublicationPair::new().unwrap();
    require_cloexec(&pair.publisher).unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let join = thread::spawn(move || publish(&mut publisher, 0, &body));
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    assert_eq!(join.join().unwrap(), ResponseKind::Ack);
    assert_eq!(receiver.latest.as_ref().unwrap().generation, 0);
}

#[test]
fn publication_complete_reject_consumes_generation_and_preserves_prior_ack() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = body.clone();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &first), ResponseKind::Ack);
        assert_eq!(publish(&mut publisher, 1, &body), ResponseKind::Ack);
        assert_eq!(
            publish(&mut publisher, 2, b"{ \"aliases\":{},\"collections\":{} }"),
            ResponseKind::Reject
        );
        drop(publisher);
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.set_nonblocking().unwrap();
    while !receiver.eof {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    join.join().unwrap();
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 1);
    assert_eq!(receiver.expected, Some(3));
}

#[test]
fn publication_partial_frame_then_eof_uses_prior_ack_written_generation() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let initial = body.clone();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &initial), ResponseKind::Ack);
        assert_eq!(publish(&mut publisher, 1, &body), ResponseKind::Ack);
        publisher
            .write_all(&encode_request_header(2, body.len()).unwrap())
            .unwrap();
        publisher.write_all(&body[..body.len() / 2]).unwrap();
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.set_nonblocking().unwrap();
    // Generation 1 cannot complete until this loop writes its ACK. Drain to
    // terminal EOF before joining so publisher and receiver make progress in
    // lockstep without relying on socket-buffer capacity or timing.
    while !receiver.eof {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    join.join().unwrap();
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 1);
}

#[test]
fn publication_validated_candidate_is_not_latest_after_partial_ack_failure_and_eof() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = body.clone();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &first), ResponseKind::Ack);
        publisher
            .write_all(&encode_request_header(1, body.len()).unwrap())
            .unwrap();
        publisher.write_all(&body).unwrap();
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.response_failure_after = Some(5);
    receiver.set_nonblocking().unwrap();
    while !receiver.eof {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    join.join().unwrap();
    assert!(receiver.poisoned);
    assert_eq!(receiver.latest.as_ref().unwrap().generation, 0);
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 0);
}

#[test]
fn publication_ack_zero_and_one_then_ordinary_publisher_death_selects_one() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = body.clone();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &first), ResponseKind::Ack);
        assert_eq!(publish(&mut publisher, 1, &body), ResponseKind::Ack);
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.set_nonblocking().unwrap();
    while !receiver.eof {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    join.join().unwrap();
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 1);
}

#[test]
fn publication_ack_zero_and_one_then_later_partial_response_failure_selects_one() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = body.clone();
    let second = body.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (continue_tx, continue_rx) = std::sync::mpsc::channel();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &first), ResponseKind::Ack);
        assert_eq!(publish(&mut publisher, 1, &second), ResponseKind::Ack);
        ready_tx.send(()).unwrap();
        continue_rx.recv().unwrap();
        publisher
            .write_all(&encode_request_header(2, body.len()).unwrap())
            .unwrap();
        publisher.write_all(&body).unwrap();
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.set_nonblocking().unwrap();
    while ready_rx.try_recv().is_err() {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    assert_eq!(receiver.latest.as_ref().unwrap().generation, 1);
    receiver.response_failure_after = Some(5);
    continue_tx.send(()).unwrap();
    while !receiver.eof {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    join.join().unwrap();
    assert!(receiver.poisoned);
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 1);
}

#[test]
fn publication_ack_response_loss_selects_the_completely_ack_written_generation() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = body.clone();
    let second = body.clone();
    let (drop_tx, drop_rx) = std::sync::mpsc::channel();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &first), ResponseKind::Ack);
        assert_eq!(publish(&mut publisher, 1, &second), ResponseKind::Ack);
        publisher
            .write_all(&encode_request_header(2, body.len()).unwrap())
            .unwrap();
        publisher.write_all(&body).unwrap();
        drop_rx.recv().unwrap();
        // Exit without reading ACK(2): caller response loss does not undo the
        // host's completely written acknowledgement.
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.set_nonblocking().unwrap();
    while receiver.latest.as_ref().map(|value| value.generation) != Some(2) {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    drop_tx.send(()).unwrap();
    while !receiver.eof {
        receiver.drain_available().unwrap();
        thread::yield_now();
    }
    join.join().unwrap();
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 2);
}

#[test]
fn publication_retained_publisher_would_block_has_no_fallback() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let mut receiver = pair.host;
    let first = body.clone();
    let join = thread::spawn(move || {
        let result = publish(&mut publisher, 0, &first);
        (publisher, result)
    });
    receiver.receive_initial().unwrap();
    let (publisher, result) = join.join().unwrap();
    assert_eq!(result, ResponseKind::Ack);
    receiver.set_nonblocking().unwrap();
    assert!(receiver.drain_available().unwrap());
    assert!(receiver.terminal_generation(true).is_err());
    drop(publisher);
}

#[test]
fn publication_fragmented_near_cap_rejection_makes_backpressure_progress() {
    let pair = PublicationPair::new().unwrap();
    let initial = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = initial.clone();
    let initial_join = thread::spawn(move || {
        let result = publish(&mut publisher, 0, &first);
        (publisher, result)
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    let (mut publisher, result) = initial_join.join().unwrap();
    assert_eq!(result, ResponseKind::Ack);
    receiver.set_nonblocking().unwrap();
    publisher.set_nonblocking(true).unwrap();

    let mut body = vec![b' '; MAX_BODY_BYTES];
    body[..initial.len()].copy_from_slice(&initial);
    let all = [
        encode_request_header(1, body.len()).unwrap().as_slice(),
        body.as_slice(),
    ]
    .concat();
    let mut sent = 0;
    while sent < all.len() {
        match publisher.write(&all[sent..]) {
            Ok(n) => sent += n,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("sending near-cap frame: {error}"),
        }
        receiver.drain_available().unwrap();
    }
    publisher.set_nonblocking(false).unwrap();
    let mut response = [0; RESPONSE_LEN];
    loop {
        receiver.drain_available().unwrap();
        match publisher.read_exact(&mut response) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(error) => panic!("reading near-cap response: {error}"),
        }
    }
    assert_eq!(
        decode_response(&response).unwrap(),
        (ResponseKind::Reject, 1)
    );
    assert_eq!(receiver.latest.as_ref().unwrap().generation, 0);
}

#[test]
fn publication_malformed_sequence_poison_requires_eof_and_preserves_prior() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let mut publisher = pair.publisher;
    let first = body.clone();
    let join = thread::spawn(move || {
        assert_eq!(publish(&mut publisher, 0, &first), ResponseKind::Ack);
        publisher
            .write_all(&encode_request_header(2, body.len()).unwrap())
            .unwrap();
        publisher.write_all(&body).unwrap();
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    receiver.set_nonblocking().unwrap();
    join.join().unwrap();
    while !receiver.eof {
        receiver.drain_available().unwrap();
    }
    assert!(receiver.poisoned);
    assert_eq!(receiver.terminal_generation(false).unwrap().generation, 0);
}

#[test]
fn publication_over_cap_header_poisoned_before_body_allocation() {
    let pair = PublicationPair::new().unwrap();
    let mut publisher = pair.publisher;
    let mut receiver = pair.host;
    let mut header = encode_request_header(0, MAX_BODY_BYTES).unwrap();
    header[14..18].copy_from_slice(&((MAX_BODY_BYTES + 1) as u32).to_be_bytes());
    let join = thread::spawn(move || publisher.write_all(&header).unwrap());
    // Initial receive is bounded and rejects before allocating a body.
    assert!(receiver.receive_initial().is_err());
    join.join().unwrap();
    assert!(receiver.latest.is_none());
}

#[test]
fn publication_fragmented_header_and_body_are_incrementally_decoded() {
    let pair = PublicationPair::new().unwrap();
    let body = LibsecretSection::default().to_canonical_bytes().unwrap();
    let header = encode_request_header(0, body.len()).unwrap();
    let mut publisher = pair.publisher;
    let join = thread::spawn(move || {
        for byte in header.into_iter().chain(body) {
            publisher.write_all(&[byte]).unwrap();
        }
        let mut response = [0; RESPONSE_LEN];
        publisher.read_exact(&mut response).unwrap();
        decode_response(&response).unwrap()
    });
    let mut receiver = pair.host;
    receiver.receive_initial().unwrap();
    assert_eq!(join.join().unwrap(), (ResponseKind::Ack, 0));
}
