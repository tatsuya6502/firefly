use super::*;

use std::sync::{Arc, RwLock};

use num_traits::Num;

use crate::environment::{self, Environment};
use crate::process::IntoProcess;

#[test]
fn with_atom_errors_badarg() {
    errors_badarg(|_| Term::str_to_atom("atom", DoNotCare).unwrap())
}

#[test]
fn with_local_reference_errors_badarg() {
    errors_badarg(|mut process| Term::local_reference(&mut process));
}

#[test]
fn with_empty_list_errors_badarg() {
    errors_badarg(|_| Term::EMPTY_LIST);
}

#[test]
fn with_list_errors_badarg() {
    errors_badarg(|mut process| list_term(&mut process));
}

#[test]
fn with_small_integer_errors_badarg() {
    errors_badarg(|mut process| 0usize.into_process(&mut process));
}

#[test]
fn with_big_integer_errors_badarg() {
    errors_badarg(|mut process| {
        <BigInt as Num>::from_str_radix("576460752303423489", 10)
            .unwrap()
            .into_process(&mut process)
    });
}

#[test]
fn with_float_errors_badarg() {
    errors_badarg(|mut process| 1.0.into_process(&mut process));
}

#[test]
fn with_local_pid_errors_badarg() {
    errors_badarg(|_| Term::local_pid(0, 0).unwrap());
}

#[test]
fn with_external_pid_errors_badarg() {
    errors_badarg(|mut process| Term::external_pid(1, 0, 0, &mut process).unwrap());
}

#[test]
fn with_tuple_errors_badarg() {
    errors_badarg(|mut process| Term::slice_to_tuple(&[], &mut process));
}

#[test]
fn with_map_errors_badarg() {
    errors_badarg(|mut process| Term::slice_to_map(&[], &mut process));
}

#[test]
fn with_heap_binary_with_start_less_than_stop_returns_list_of_bytes() {
    let environment_rw_lock: Arc<RwLock<Environment>> = Default::default();
    let process_rw_lock = environment::process(Arc::clone(&environment_rw_lock));
    let mut process = process_rw_lock.write().unwrap();
    let binary = Term::slice_to_binary(&[0, 1, 2], &mut process);

    assert_eq!(
        erlang::binary_to_list_3(
            binary,
            2.into_process(&mut process),
            3.into_process(&mut process),
            &mut process
        ),
        Ok(Term::cons(
            1.into_process(&mut process),
            Term::EMPTY_LIST,
            &mut process
        ))
    );
}

#[test]
fn with_heap_binary_with_start_equal_to_stop_returns_list_of_single_byte() {
    let environment_rw_lock: Arc<RwLock<Environment>> = Default::default();
    let process_rw_lock = environment::process(Arc::clone(&environment_rw_lock));
    let mut process = process_rw_lock.write().unwrap();
    let binary = Term::slice_to_binary(&[0, 1, 2], &mut process);

    assert_eq!(
        erlang::binary_to_list_3(
            binary,
            2.into_process(&mut process),
            2.into_process(&mut process),
            &mut process
        ),
        Ok(Term::cons(
            1.into_process(&mut process),
            Term::EMPTY_LIST,
            &mut process
        ))
    );
}

#[test]
fn with_heap_binary_with_start_greater_than_stop_errors_badarg() {
    let environment_rw_lock: Arc<RwLock<Environment>> = Default::default();
    let process_rw_lock = environment::process(Arc::clone(&environment_rw_lock));
    let mut process = process_rw_lock.write().unwrap();
    let binary = Term::slice_to_binary(&[0, 1, 2], &mut process);

    assert_badarg!(erlang::binary_to_list_3(
        binary,
        3.into_process(&mut process),
        2.into_process(&mut process),
        &mut process
    ));
}

#[test]
fn with_subbinary_with_start_less_than_stop_returns_list_of_bytes() {
    let environment_rw_lock: Arc<RwLock<Environment>> = Default::default();
    let process_rw_lock = environment::process(Arc::clone(&environment_rw_lock));
    let mut process = process_rw_lock.write().unwrap();
    // <<1::1, 0, 1, 2>>
    let original = Term::slice_to_binary(&[128, 0, 129, 0b0000_0000], &mut process);
    let binary = Term::subbinary(original, 0, 1, 3, 0, &mut process);

    assert_eq!(
        erlang::binary_to_list_3(
            binary,
            2.into_process(&mut process),
            3.into_process(&mut process),
            &mut process
        ),
        Ok(Term::cons(
            1.into_process(&mut process),
            Term::EMPTY_LIST,
            &mut process
        ))
    );
}

#[test]
fn with_subbinary_with_start_equal_to_stop_returns_list_of_single_byte() {
    let environment_rw_lock: Arc<RwLock<Environment>> = Default::default();
    let process_rw_lock = environment::process(Arc::clone(&environment_rw_lock));
    let mut process = process_rw_lock.write().unwrap();
    // <<1::1, 0, 1, 2>>
    let original = Term::slice_to_binary(&[128, 0, 129, 0b0000_0000], &mut process);
    let binary = Term::subbinary(original, 0, 1, 3, 0, &mut process);

    assert_eq!(
        erlang::binary_to_list_3(
            binary,
            2.into_process(&mut process),
            2.into_process(&mut process),
            &mut process
        ),
        Ok(Term::cons(
            1.into_process(&mut process),
            Term::EMPTY_LIST,
            &mut process
        ))
    );
}

#[test]
fn with_subbinary_with_start_greater_than_stop_errors_badarg() {
    let environment_rw_lock: Arc<RwLock<Environment>> = Default::default();
    let process_rw_lock = environment::process(Arc::clone(&environment_rw_lock));
    let mut process = process_rw_lock.write().unwrap();
    // <<1::1, 0, 1, 2>>
    let original = Term::slice_to_binary(&[128, 0, 129, 0b0000_0000], &mut process);
    let binary = Term::subbinary(original, 0, 1, 3, 0, &mut process);

    assert_badarg!(erlang::binary_to_list_3(
        binary,
        3.into_process(&mut process),
        2.into_process(&mut process),
        &mut process
    ));
}

fn errors_badarg<F>(binary: F)
where
    F: FnOnce(&mut Process) -> Term,
{
    super::errors_badarg(|mut process| {
        let start = 2.into_process(&mut process);
        let stop = 3.into_process(&mut process);

        erlang::binary_to_list_3(binary(&mut process), start, stop, &mut process)
    });
}
