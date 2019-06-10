use super::*;

mod with_0_bit_subbinary;
mod with_1_bit_subbinary;
mod with_2_bit_subbinary;
mod with_3_bit_subbinary;
mod with_4_bit_subbinary;
mod with_5_bit_subbinary;
mod with_6_bit_subbinary;
mod with_7_bit_subbinary;
mod with_byte;
mod with_heap_binary;

#[test]
fn without_byte_bitstring_or_list_element_errors_badarg() {
    with_process_arc(|arc_process| {
        TestRunner::new(Config::with_source_file(file!()))
            .run(
                &is_not_byte_bitstring_nor_list(arc_process.clone())
                    .prop_map(|element| Term::cons(element, Term::EMPTY_LIST, &arc_process)),
                |list| {
                    prop_assert_eq!(
                        erlang::list_to_bitstring_1(list, &arc_process),
                        Err(badarg!())
                    );

                    Ok(())
                },
            )
            .unwrap();
    });
}

#[test]
fn with_empty_list_returns_empty_binary() {
    with_process(|process| {
        let iolist = Term::cons(Term::EMPTY_LIST, Term::EMPTY_LIST, &process);

        assert_eq!(
            erlang::list_to_bitstring_1(iolist, &process),
            Ok(Term::slice_to_binary(&[], &process))
        );
    })
}

fn is_not_byte_bitstring_nor_list(arc_process: Arc<Process>) -> BoxedStrategy<Term> {
    strategy::term(arc_process.clone())
        .prop_filter("Element must not be a binary or byte", move |element| {
            !(element.is_bitstring()
                || (element.is_integer()
                    && &0.into_process(&arc_process) <= element
                    && element <= &256_isize.into_process(&arc_process))
                || element.is_list())
        })
        .boxed()
}
