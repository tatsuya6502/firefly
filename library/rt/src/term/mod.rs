mod atom;
mod binary;
mod closure;
mod index;
mod list;
mod map;
mod node;
mod opaque;
mod pid;
mod port;
mod reference;
mod tuple;

pub use self::atom::{atoms, Atom, AtomData};
pub use self::binary::*;
pub use self::closure::Closure;
pub use self::index::{NonPrimitiveIndex, OneBasedIndex, TupleIndex, ZeroBasedIndex};
pub use self::list::{Cons, ImproperList, ListBuilder};
pub use self::map::Map;
pub use self::node::Node;
pub use self::opaque::{OpaqueTerm, TermType};
pub use self::pid::{Pid, ProcessId};
pub use self::port::{Port, PortId};
pub use self::reference::{Reference, ReferenceId};
pub use self::tuple::Tuple;

pub use firefly_number::{BigInt, Float, Integer, Number};
use firefly_number::{DivisionError, InvalidArithmeticError, Sign, ToPrimitive};

use alloc::alloc::{AllocError, Layout};
use core::convert::AsRef;
use core::fmt;
use core::ptr::NonNull;

use anyhow::anyhow;
use firefly_alloc::fragment::HeapFragment;
use firefly_alloc::gc::GcBox;
use firefly_alloc::heap::Heap;
use firefly_alloc::rc::{Rc, Weak};
use firefly_binary::{Binary, Bitstring, Encoding};

use crate::cmp::ExactEq;

/// `Term` is two things:
///
/// * An enumeration of the types that can be represented as Erlang values,
/// unifying them under a single conceptual value type, i.e. term. As such,
/// Term is how comparisons/equality/etc. are defined between value types.
///
/// * The decoded form of `OpaqueTerm`. `OpaqueTerm` is a compact encoding intended to
/// guarantee that passing around a term value in Erlang code is always the same size (i.e.
/// u64 on 64-bit systems). This invariant is not needed in Rust, and it is more ergonomic
/// and performant to pass around a less compact represenetation. However, we want to preserve
/// the `Copy`-ability of the `OpaqueTerm` representation, so `Term` still requires some care
/// when performing certain operations such as cloning and dropping, where the underlying type
/// may have additional invariants that `Term` does not know about. For example, we use trait
/// objects for the various binary types to keep things simple when working with them as terms,
/// but when garbage collecting, we must make sure that we operate on the concrete type,
/// as some binaries are heap-allocated while others are reference-counted, and the semantics
/// of how those are collected are different.
///
/// See notes on the individual variants for why a specific representation was chosen for that
/// variant.
#[derive(Debug, Copy, Clone, Hash)]
#[repr(C)]
pub enum Term {
    None,
    Nil,
    Bool(bool),
    Atom(Atom),
    Int(i64),
    BigInt(GcBox<BigInt>),
    Float(Float),
    /// Cons cells are not allocated via a box type, but are instead differentiated
    /// from other boxed terms by the pointer tagging scheme
    Cons(NonNull<Cons>),
    /// Tuples, like Cons, is not allocated via a box type, but has its own pointer tag scheme
    Tuple(NonNull<Tuple>),
    Map(GcBox<Map>),
    Closure(GcBox<Closure>),
    Pid(GcBox<Pid>),
    Port(GcBox<Port>),
    Reference(GcBox<Reference>),
    HeapBinary(GcBox<BinaryData>),
    RcBinary(Weak<BinaryData>),
    RefBinary(GcBox<BitSlice>),
    ConstantBinary(&'static BinaryData),
}
impl Term {
    pub fn clone_to_fragment(self) -> Result<(Self, NonNull<HeapFragment>), AllocError> {
        let layout = self.layout();
        let frag = HeapFragment::new(layout, None)?;
        let term = self.clone_to_heap(unsafe { frag.as_ref() })?;
        Ok((term, frag))
    }

    pub fn clone_to_heap<H: Heap>(self, heap: H) -> Result<Self, AllocError> {
        let cloned = match self {
            Self::None => Self::None,
            Self::Nil => Self::Nil,
            Self::Bool(b) => Self::Bool(b),
            Self::Atom(a) => Self::Atom(a),
            Self::Int(i) => Self::Int(i),
            Self::Float(f) => Self::Float(f),
            Self::BigInt(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::BigInt(boxed)
                } else {
                    let mut empty = GcBox::new_uninit_in(heap)?;
                    empty.write((&*boxed).clone());
                    Self::BigInt(unsafe { empty.assume_init() })
                }
            }
            Self::Cons(ptr) => {
                if heap.contains(ptr.as_ptr()) {
                    Self::Cons(ptr)
                } else {
                    let old = unsafe { ptr.as_ref() };
                    let cons = Cons::new_in(heap)?;
                    unsafe {
                        cons.as_uninit_mut().write(*old);
                    }
                    Self::Cons(cons)
                }
            }
            Self::Tuple(ptr) => {
                if heap.contains(ptr.as_ptr()) {
                    Self::Tuple(ptr)
                } else {
                    let tuple = unsafe { ptr.as_ref() };
                    Self::Tuple(Tuple::from_slice(tuple.as_slice(), heap)?)
                }
            }
            Self::Map(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::Map(boxed)
                } else {
                    Self::Map(GcBox::new_in((&*boxed).clone(), heap)?)
                }
            }
            Self::Closure(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::Closure(boxed)
                } else {
                    let mut cloned = GcBox::<Closure>::with_capacity_in(boxed.env_size(), heap)?;
                    cloned.copy_from(&boxed);
                    Self::Closure(cloned)
                }
            }
            Self::Pid(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::Pid(boxed)
                } else {
                    Self::Pid(GcBox::new_in((&*boxed).clone(), heap)?)
                }
            }
            Self::Port(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::Port(boxed)
                } else {
                    Self::Port(GcBox::new_in((&*boxed).clone(), heap)?)
                }
            }
            Self::Reference(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::Reference(boxed)
                } else {
                    Self::Reference(GcBox::new_in((&*boxed).clone(), heap)?)
                }
            }
            Self::HeapBinary(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::HeapBinary(boxed)
                } else {
                    let bytes = boxed.as_bytes();
                    let mut cloned = GcBox::<BinaryData>::with_capacity_in(bytes.len(), heap)?;
                    {
                        unsafe {
                            cloned.set_flags(boxed.flags());
                        }
                        cloned.copy_from_slice(bytes);
                    }
                    Self::HeapBinary(cloned)
                }
            }
            Self::RcBinary(ref weak) => Self::RcBinary(Rc::into_weak(Weak::upgrade(weak))),
            Self::RefBinary(boxed) => {
                if heap.contains(GcBox::as_ptr(&boxed)) {
                    Self::RefBinary(boxed)
                } else {
                    Self::RefBinary(GcBox::<BitSlice>::new_in((&*boxed).clone(), heap)?)
                }
            }
            Self::ConstantBinary(bytes) => Self::ConstantBinary(bytes),
        };
        Ok(cloned)
    }

    pub fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            _ => false,
        }
    }

    pub fn is_nil(&self) -> bool {
        match self {
            Self::Nil => true,
            _ => false,
        }
    }

    pub fn as_cons(&self) -> Option<&Cons> {
        match self {
            Self::Cons(ptr) => Some(unsafe { ptr.as_ref() }),
            _ => None,
        }
    }

    pub fn as_tuple(&self) -> Option<&Tuple> {
        match self {
            Self::Tuple(ptr) => Some(unsafe { ptr.as_ref() }),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Self::Map(map) => Some(map.as_ref()),
            _ => None,
        }
    }

    pub fn as_closure(&self) -> Option<&Closure> {
        match self {
            Self::Closure(fun) => Some(fun.as_ref()),
            _ => None,
        }
    }
    pub fn as_pid(&self) -> Option<&Pid> {
        match self {
            Self::Pid(pid) => Some(pid.as_ref()),
            _ => None,
        }
    }

    pub fn as_port(&self) -> Option<&Port> {
        match self {
            Self::Port(port) => Some(port.as_ref()),
            _ => None,
        }
    }

    pub fn as_reference(&self) -> Option<&Reference> {
        match self {
            Self::Reference(r) => Some(r.as_ref()),
            _ => None,
        }
    }

    pub fn as_bitstring(&self) -> Option<&dyn Bitstring> {
        match self {
            Self::HeapBinary(boxed) => Some(boxed),
            Self::RcBinary(boxed) => Some(boxed),
            Self::RefBinary(boxed) => Some(boxed),
            Self::ConstantBinary(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn is_bitstring(&self) -> bool {
        match self {
            Self::HeapBinary(_)
            | Self::RcBinary(_)
            | Self::RefBinary(_)
            | Self::ConstantBinary(_) => true,
            _ => false,
        }
    }

    #[inline]
    pub fn as_char(self) -> Result<char, ()> {
        self.try_into()
    }

    pub fn exact_eq(&self, other: &Self) -> bool {
        // With exception of bitstring variants, if the discriminant is different, the
        // types can never be exactly equal
        if core::mem::discriminant(self) != core::mem::discriminant(other) {
            if self.is_bitstring() && other.is_bitstring() {
                return self.eq(other);
            }
            return false;
        }
        self.eq(other)
    }

    /// Returns a Layout which can be used to allocate sufficient memory to
    /// hold this term and its associated data, including any references.
    pub fn layout(&self) -> Layout {
        match self {
            Self::None
            | Self::Nil
            | Self::Bool(_)
            | Self::Atom(_)
            | Self::Int(_)
            | Self::Float(_)
            | Self::ConstantBinary(_) => Layout::new::<OpaqueTerm>(),
            Self::BigInt(_) => {
                let (base, _) = Layout::new::<GcBox<BigInt>>()
                    .extend(Layout::new::<BigInt>())
                    .unwrap();
                base.pad_to_align()
            }
            Self::Cons(_) => Layout::new::<Cons>(),
            Self::Tuple(t) => {
                let tuple = unsafe { t.as_ref() };
                let base = Layout::for_value(tuple);
                tuple.iter().fold(base, |layout, element| {
                    let (extended, _) = layout.extend(element.layout()).unwrap();
                    extended.pad_to_align()
                })
            }
            Self::Map(map) => {
                let (base, _) = Layout::new::<GcBox<Map>>()
                    .extend(Layout::new::<Map>())
                    .unwrap();
                map.iter().fold(base, |layout, (k, v)| {
                    let (extended, _) = layout.extend(k.layout()).unwrap();
                    let (extended, _) = extended.pad_to_align().extend(v.layout()).unwrap();
                    extended.pad_to_align()
                })
            }
            Self::Closure(fun) => {
                let (base, _) = Layout::new::<GcBox<Closure>>()
                    .extend(Layout::for_value(fun.as_ref()))
                    .unwrap();
                fun.env().iter().copied().fold(base, |layout, opaque| {
                    let term: Term = opaque.into();
                    let (extended, _) = layout.extend(term.layout()).unwrap();
                    extended.pad_to_align()
                })
            }
            Self::Pid(_) => {
                let (base, _) = Layout::new::<GcBox<Pid>>()
                    .extend(Layout::new::<Pid>())
                    .unwrap();
                base.pad_to_align()
            }
            Self::Port(_) => {
                let (base, _) = Layout::new::<GcBox<Port>>()
                    .extend(Layout::new::<Port>())
                    .unwrap();
                base.pad_to_align()
            }
            Self::Reference(_) => {
                let (base, _) = Layout::new::<GcBox<Reference>>()
                    .extend(Layout::new::<Reference>())
                    .unwrap();
                base.pad_to_align()
            }
            Self::HeapBinary(bin) => {
                let (base, _) = Layout::new::<GcBox<BinaryData>>()
                    .extend(Layout::for_value(bin.as_ref()))
                    .unwrap();
                base.pad_to_align()
            }
            Self::RcBinary(_) => Layout::new::<Weak<BinaryData>>(),
            Self::RefBinary(_) => {
                let (base, _) = Layout::new::<GcBox<BitSlice>>()
                    .extend(Layout::new::<BitSlice>())
                    .unwrap();
                base.pad_to_align()
            }
        }
    }
}
impl From<bool> for Term {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}
impl From<Atom> for Term {
    fn from(a: Atom) -> Self {
        Self::Atom(a)
    }
}
impl TryFrom<usize> for Term {
    type Error = ();
    #[inline]
    fn try_from(i: usize) -> Result<Self, ()> {
        let i: i64 = i.try_into().map_err(|_| ())?;
        i.try_into()
    }
}
impl TryFrom<isize> for Term {
    type Error = ();
    #[inline]
    fn try_from(i: isize) -> Result<Self, ()> {
        (i as i64).try_into()
    }
}
impl TryFrom<i64> for Term {
    type Error = ();
    fn try_from(i: i64) -> Result<Self, ()> {
        if OpaqueTerm::is_small_integer(i) {
            Ok(Self::Int(i))
        } else {
            Err(())
        }
    }
}
impl From<GcBox<BigInt>> for Term {
    fn from(i: GcBox<BigInt>) -> Self {
        Self::BigInt(i)
    }
}
impl From<f64> for Term {
    fn from(f: f64) -> Self {
        Self::Float(f.into())
    }
}
impl From<Float> for Term {
    fn from(f: Float) -> Self {
        Self::Float(f)
    }
}
impl From<NonNull<Cons>> for Term {
    fn from(term: NonNull<Cons>) -> Self {
        Self::Cons(term)
    }
}
impl From<NonNull<Tuple>> for Term {
    fn from(term: NonNull<Tuple>) -> Self {
        Self::Tuple(term)
    }
}
impl From<GcBox<Map>> for Term {
    fn from(term: GcBox<Map>) -> Self {
        Self::Map(term)
    }
}
impl From<GcBox<Closure>> for Term {
    fn from(term: GcBox<Closure>) -> Self {
        Self::Closure(term)
    }
}
impl From<GcBox<Pid>> for Term {
    fn from(term: GcBox<Pid>) -> Self {
        Self::Pid(term)
    }
}
impl From<GcBox<Port>> for Term {
    fn from(term: GcBox<Port>) -> Self {
        Self::Port(term)
    }
}
impl From<GcBox<Reference>> for Term {
    fn from(term: GcBox<Reference>) -> Self {
        Self::Reference(term)
    }
}
impl From<GcBox<BinaryData>> for Term {
    fn from(term: GcBox<BinaryData>) -> Self {
        Self::HeapBinary(term)
    }
}
impl From<Weak<BinaryData>> for Term {
    fn from(term: Weak<BinaryData>) -> Self {
        Self::RcBinary(term)
    }
}
impl From<GcBox<BitSlice>> for Term {
    fn from(term: GcBox<BitSlice>) -> Self {
        Self::RefBinary(term)
    }
}
impl From<&'static BinaryData> for Term {
    fn from(term: &'static BinaryData) -> Self {
        Self::ConstantBinary(term)
    }
}
impl TryInto<bool> for Term {
    type Error = ();
    fn try_into(self) -> Result<bool, Self::Error> {
        match self {
            Self::Bool(b) => Ok(b),
            Self::Atom(a) if a.is_boolean() => Ok(a.as_boolean()),
            _ => Err(()),
        }
    }
}
impl TryInto<Atom> for Term {
    type Error = ();
    fn try_into(self) -> Result<Atom, Self::Error> {
        match self {
            Self::Atom(a) => Ok(a),
            Self::Bool(b) => Ok(b.into()),
            _ => Err(()),
        }
    }
}
impl TryInto<char> for Term {
    type Error = ();
    fn try_into(self) -> Result<char, Self::Error> {
        const MAX: i64 = char::MAX as u32 as i64;

        let i: i64 = self.try_into()?;

        if i >= 0 && i <= MAX {
            (i as u32).try_into().map_err(|_| ())
        } else {
            Err(())
        }
    }
}
impl TryInto<i64> for Term {
    type Error = ();
    #[inline]
    fn try_into(self) -> Result<i64, Self::Error> {
        match self {
            Self::Int(i) => Ok(i),
            Self::BigInt(i) => match i.to_i64() {
                Some(i) => Ok(i),
                None => Err(()),
            },
            _ => Err(()),
        }
    }
}
impl TryInto<Integer> for Term {
    type Error = ();
    #[inline]
    fn try_into(self) -> Result<Integer, Self::Error> {
        match self {
            Self::Int(i) => Ok(Integer::Small(i)),
            Self::BigInt(i) => Ok(Integer::Big((*i).clone())),
            _ => Err(()),
        }
    }
}
impl TryInto<Number> for Term {
    type Error = ();
    #[inline]
    fn try_into(self) -> Result<Number, Self::Error> {
        match self {
            Self::Int(i) => Ok(Number::Integer(Integer::Small(i))),
            Self::BigInt(i) => Ok(Number::Integer(Integer::Big((*i).clone()))),
            Self::Float(f) => Ok(Number::Float(f)),
            _ => Err(()),
        }
    }
}
impl TryInto<NonNull<Cons>> for Term {
    type Error = ();
    fn try_into(self) -> Result<NonNull<Cons>, Self::Error> {
        match self {
            Self::Cons(c) => Ok(c),
            _ => Err(()),
        }
    }
}
impl TryInto<NonNull<Tuple>> for Term {
    type Error = ();
    fn try_into(self) -> Result<NonNull<Tuple>, Self::Error> {
        match self {
            Self::Tuple(t) => Ok(t),
            _ => Err(()),
        }
    }
}
impl TryInto<GcBox<BigInt>> for Term {
    type Error = ();
    fn try_into(self) -> Result<GcBox<BigInt>, Self::Error> {
        match self {
            Self::BigInt(i) => Ok(i),
            _ => Err(()),
        }
    }
}
impl TryInto<f64> for Term {
    type Error = ();
    fn try_into(self) -> Result<f64, Self::Error> {
        match self {
            Self::Float(f) => Ok(f.into()),
            _ => Err(()),
        }
    }
}
impl TryInto<Float> for Term {
    type Error = ();
    fn try_into(self) -> Result<Float, Self::Error> {
        match self {
            Self::Float(f) => Ok(f),
            _ => Err(()),
        }
    }
}
// Support converting from atom terms to `Encoding` type
impl TryInto<Encoding> for Term {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Encoding, Self::Error> {
        match self {
            Self::Atom(a) => a.as_str().parse(),
            other => Err(anyhow!(
                "invalid encoding name: expected atom; got {}",
                &other
            )),
        }
    }
}
impl AsRef<Term> for Term {
    #[inline(always)]
    fn as_ref(&self) -> &Term {
        self
    }
}
impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => write!(f, "NONE"),
            Self::Nil => f.write_str("[]"),
            Self::Bool(term) => write!(f, "{}", term),
            Self::Atom(term) => write!(f, "{}", term),
            Self::Int(term) => write!(f, "{}", term),
            Self::BigInt(term) => write!(f, "{}", term),
            Self::Float(term) => write!(f, "{}", term),
            Self::Cons(ptr) => write!(f, "{}", unsafe { ptr.as_ref() }),
            Self::Tuple(ptr) => write!(f, "{}", unsafe { ptr.as_ref() }),
            Self::Map(boxed) => write!(f, "{}", boxed),
            Self::Closure(boxed) => write!(f, "{}", boxed),
            Self::Pid(boxed) => write!(f, "{}", boxed),
            Self::Port(boxed) => write!(f, "{}", boxed),
            Self::Reference(boxed) => write!(f, "{}", boxed),
            Self::HeapBinary(boxed) => write!(f, "{}", boxed),
            Self::RcBinary(boxed) => write!(f, "{}", boxed),
            Self::RefBinary(boxed) => write!(f, "{}", boxed),
            Self::ConstantBinary(bytes) => write!(f, "{}", bytes),
        }
    }
}
impl Eq for Term {}
impl PartialEq for Term {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Self::None => other.is_none(),
            Self::Nil => other.is_nil(),
            Self::Bool(x) => match other {
                Self::Bool(y) => x == y,
                _ => false,
            },
            Self::Atom(x) => match other {
                Self::Atom(y) => x == y,
                _ => false,
            },
            Self::Int(x) => match other {
                Self::Int(y) => x == y,
                Self::BigInt(y) => match y.to_i64() {
                    Some(ref y) => x == y,
                    None => false,
                },
                Self::Float(y) => y == x,
                _ => false,
            },
            Self::BigInt(x) => match other {
                Self::Int(y) => match x.to_i64() {
                    Some(ref x) => x == y,
                    None => false,
                },
                Self::BigInt(y) => x == y,
                Self::Float(y) => y == (&**x),
                _ => false,
            },
            Self::Float(x) => match other {
                Self::Float(y) => x == y,
                Self::Int(y) => x == y,
                Self::BigInt(y) => x == (&**y),
                _ => false,
            },
            Self::Cons(x) => match other {
                Self::Cons(y) => unsafe { x.as_ref().eq(y.as_ref()) },
                _ => false,
            },
            Self::Tuple(x) => match other {
                Self::Tuple(y) => unsafe { x.as_ref().eq(y.as_ref()) },
                _ => false,
            },
            Self::Map(x) => match other {
                Self::Map(y) => x == y,
                _ => false,
            },
            Self::Closure(x) => match other {
                Self::Closure(y) => x == y,
                _ => false,
            },
            Self::Pid(x) => match other {
                Self::Pid(y) => x == y,
                _ => false,
            },
            Self::Port(x) => match other {
                Self::Port(y) => x == y,
                _ => false,
            },
            Self::Reference(x) => match other {
                Self::Reference(y) => x == y,
                _ => false,
            },
            Self::HeapBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_ref().eq(y),
                Self::HeapBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RcBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RefBinary(y) => x.as_ref().eq(y.as_ref()),
                _ => false,
            },
            Self::RcBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_ref().eq(y),
                Self::HeapBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RcBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RefBinary(y) => x.as_ref().eq(y.as_ref()),
                _ => false,
            },
            Self::RefBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_ref().eq(y),
                Self::HeapBinary(y) => x.as_ref().eq(y),
                Self::RcBinary(y) => x.as_ref().eq(y),
                Self::RefBinary(y) => x.as_ref().eq(y),
                _ => false,
            },
            Self::ConstantBinary(x) => match other {
                Self::ConstantBinary(y) => x.eq(y),
                Self::HeapBinary(y) => x.as_bytes().eq(y.as_bytes()),
                Self::RcBinary(y) => x.as_bytes().eq(y.as_bytes()),
                Self::RefBinary(y) => y.as_ref().eq(x),
                _ => false,
            },
        }
    }
}
impl ExactEq for Term {
    fn exact_eq(&self, other: &Self) -> bool {
        match self {
            Self::None => other.is_none(),
            Self::Nil => other.is_nil(),
            Self::Bool(x) => match other {
                Self::Bool(y) => x == y,
                _ => false,
            },
            Self::Atom(x) => match other {
                Self::Atom(y) => x == y,
                _ => false,
            },
            Self::Int(x) => match other {
                Self::Int(y) => x == y,
                Self::BigInt(y) => match y.to_i64() {
                    Some(ref y) => x == y,
                    None => false,
                },
                _ => false,
            },
            Self::BigInt(x) => match other {
                Self::Int(y) => match x.to_i64() {
                    Some(ref x) => x == y,
                    None => false,
                },
                Self::BigInt(y) => x == y,
                _ => false,
            },
            Self::Float(x) => match other {
                Self::Float(y) => x == y,
                _ => false,
            },
            Self::Cons(x) => match other {
                Self::Cons(y) => unsafe { x.as_ref().exact_eq(y.as_ref()) },
                _ => false,
            },
            Self::Tuple(x) => match other {
                Self::Tuple(y) => unsafe { x.as_ref().exact_eq(y.as_ref()) },
                _ => false,
            },
            Self::Map(x) => match other {
                Self::Map(y) => x.as_ref().exact_eq(y.as_ref()),
                _ => false,
            },
            Self::Closure(x) => match other {
                Self::Closure(y) => x.as_ref().exact_eq(y.as_ref()),
                _ => false,
            },
            Self::Pid(x) => match other {
                Self::Pid(y) => x.as_ref().exact_eq(y.as_ref()),
                _ => false,
            },
            Self::Port(x) => match other {
                Self::Port(y) => x.as_ref().exact_eq(y.as_ref()),
                _ => false,
            },
            Self::Reference(x) => match other {
                Self::Reference(y) => x.as_ref().exact_eq(y.as_ref()),
                _ => false,
            },
            Self::HeapBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_ref().eq(y),
                Self::HeapBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RcBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RefBinary(y) => x.as_ref().eq(y.as_ref()),
                _ => false,
            },
            Self::RcBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_ref().eq(y),
                Self::HeapBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RcBinary(y) => x.as_ref().eq(y.as_ref()),
                Self::RefBinary(y) => x.as_ref().eq(y.as_ref()),
                _ => false,
            },
            Self::RefBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_ref().eq(y),
                Self::HeapBinary(y) => x.as_ref().eq(y),
                Self::RcBinary(y) => x.as_ref().eq(y),
                Self::RefBinary(y) => x.as_ref().eq(y),
                _ => false,
            },
            Self::ConstantBinary(x) => match other {
                Self::ConstantBinary(y) => x.eq(y),
                Self::HeapBinary(y) => x.as_bytes().eq(y.as_bytes()),
                Self::RcBinary(y) => x.as_bytes().eq(y.as_bytes()),
                Self::RefBinary(y) => y.as_ref().eq(x),
                _ => false,
            },
        }
    }

    #[inline]
    fn exact_ne(&self, other: &Self) -> bool {
        !self.exact_eq(other)
    }
}
impl PartialOrd for Term {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Term {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        match self {
            // None is always least
            Self::None => {
                if other.is_none() {
                    Ordering::Equal
                } else {
                    Ordering::Less
                }
            }
            // Numbers are smaller than all other terms, using whichever type has the highest precision.
            // We need comparison order to preserve the ExactEq semantics, so equality between integers/floats
            // is broken by sorting floats first due to their greater precision in most cases
            Self::Int(x) => match other {
                Self::None => Ordering::Greater,
                Self::Int(y) => x.cmp(y),
                Self::BigInt(y) => match y.to_i64() {
                    Some(y) => x.cmp(&y),
                    None if y.sign() == Sign::Minus => Ordering::Greater,
                    None => Ordering::Less,
                },
                Self::Float(y) => match y.partial_cmp(x).unwrap().reverse() {
                    Ordering::Equal => Ordering::Greater,
                    other => other,
                },
                _ => Ordering::Less,
            },
            Self::BigInt(x) => match other {
                Self::None => Ordering::Greater,
                Self::Int(y) => match x.to_i64() {
                    Some(x) => x.cmp(&y),
                    None if x.sign() == Sign::Minus => Ordering::Less,
                    None => Ordering::Greater,
                },
                Self::BigInt(y) => (&**x).cmp(&**y),
                Self::Float(y) => match y.partial_cmp(&**x).unwrap().reverse() {
                    Ordering::Equal => Ordering::Greater,
                    other => other,
                },
                _ => Ordering::Less,
            },
            Self::Float(x) => match other {
                Self::None => Ordering::Greater,
                Self::Float(y) => x.partial_cmp(y).unwrap(),
                Self::Int(y) => match x.partial_cmp(y).unwrap() {
                    Ordering::Equal => Ordering::Less,
                    other => other,
                },
                Self::BigInt(y) => match x.partial_cmp(&**y).unwrap() {
                    Ordering::Equal => Ordering::Less,
                    other => other,
                },
                _ => Ordering::Less,
            },
            Self::Bool(x) => match other {
                Self::Bool(y) => x.cmp(y),
                Self::Atom(a) if a.is_boolean() => x.cmp(&a.as_boolean()),
                Self::Atom(_) => Ordering::Less,
                Self::None | Self::Int(_) | Self::BigInt(_) | Self::Float(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Atom(x) => match other {
                Self::Atom(y) => x.cmp(y),
                Self::Bool(y) if x.is_boolean() => x.as_boolean().cmp(y),
                Self::Bool(_) => Ordering::Greater,
                Self::None | Self::Int(_) | Self::BigInt(_) | Self::Float(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Reference(x) => match other {
                Self::Reference(y) => x.cmp(y),
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Closure(x) => match other {
                Self::Closure(y) => x.cmp(y),
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Port(x) => match other {
                Self::Port(y) => x.cmp(y),
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_)
                | Self::Closure(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Pid(x) => match other {
                Self::Pid(y) => x.cmp(y),
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_)
                | Self::Closure(_)
                | Self::Port(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Tuple(x) => match other {
                Self::Tuple(y) => unsafe { x.as_ref().cmp(y.as_ref()) },
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_)
                | Self::Closure(_)
                | Self::Port(_)
                | Self::Pid(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Map(x) => match other {
                Self::Map(y) => x.cmp(y),
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_)
                | Self::Closure(_)
                | Self::Port(_)
                | Self::Pid(_)
                | Self::Tuple(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Nil => match other {
                Self::Nil => Ordering::Equal,
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_)
                | Self::Closure(_)
                | Self::Port(_)
                | Self::Pid(_)
                | Self::Tuple(_)
                | Self::Map(_) => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::Cons(x) => match other {
                Self::Cons(y) => unsafe { x.as_ref().cmp(y.as_ref()) },
                Self::None
                | Self::Int(_)
                | Self::BigInt(_)
                | Self::Float(_)
                | Self::Bool(_)
                | Self::Atom(_)
                | Self::Reference(_)
                | Self::Closure(_)
                | Self::Port(_)
                | Self::Pid(_)
                | Self::Tuple(_)
                | Self::Map(_)
                | Self::Nil => Ordering::Greater,
                _ => Ordering::Less,
            },
            Self::HeapBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_bytes().cmp(y.as_bytes()),
                Self::HeapBinary(y) => x.cmp(y),
                Self::RcBinary(y) => (&**x).partial_cmp(y).unwrap(),
                Self::RefBinary(y) => (&**x).partial_cmp(y).unwrap(),
                _ => Ordering::Greater,
            },
            Self::RcBinary(x) => match other {
                Self::ConstantBinary(y) => x.as_bytes().cmp(y.as_bytes()),
                Self::HeapBinary(y) => (&**x).partial_cmp(y).unwrap(),
                Self::RcBinary(y) => x.cmp(y),
                Self::RefBinary(y) => (&**x).partial_cmp(y).unwrap(),
                _ => Ordering::Greater,
            },
            Self::RefBinary(x) => match other {
                Self::ConstantBinary(y) => (&**x).partial_cmp(y).unwrap(),
                Self::HeapBinary(y) => (&**x).partial_cmp(y).unwrap(),
                Self::RcBinary(y) => (&**x).partial_cmp(y).unwrap(),
                Self::RefBinary(y) => x.cmp(y),
                _ => Ordering::Greater,
            },
            Self::ConstantBinary(x) => match other {
                Self::ConstantBinary(y) => x.cmp(y),
                Self::HeapBinary(y) => x.as_bytes().cmp(y.as_bytes()),
                Self::RcBinary(y) => x.as_bytes().cmp(y.as_bytes()),
                Self::RefBinary(y) => (&**y).partial_cmp(x).unwrap().reverse(),
                _ => Ordering::Greater,
            },
        }
    }
}
impl core::ops::Add for Term {
    type Output = Result<Number, InvalidArithmeticError>;

    fn add(self, rhs: Self) -> Self::Output {
        let lhs: Number = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Number = rhs.try_into().map_err(|_| InvalidArithmeticError)?;
        (lhs + rhs).map_err(|_| InvalidArithmeticError)
    }
}
impl core::ops::Sub for Term {
    type Output = Result<Number, InvalidArithmeticError>;

    fn sub(self, rhs: Self) -> Self::Output {
        let lhs: Number = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Number = rhs.try_into().map_err(|_| InvalidArithmeticError)?;
        (lhs - rhs).map_err(|_| InvalidArithmeticError)
    }
}
impl core::ops::Mul for Term {
    type Output = Result<Number, InvalidArithmeticError>;

    fn mul(self, rhs: Self) -> Self::Output {
        let lhs: Number = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Number = rhs.try_into().map_err(|_| InvalidArithmeticError)?;
        (lhs * rhs).map_err(|_| InvalidArithmeticError)
    }
}
impl core::ops::Div for Term {
    type Output = Result<Result<Number, DivisionError>, InvalidArithmeticError>;

    fn div(self, rhs: Self) -> Self::Output {
        let lhs: Number = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Number = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        match (lhs, rhs) {
            (Number::Integer(lhs), Number::Integer(rhs)) => Ok((lhs / rhs).map(Number::Integer)),
            (Number::Float(lhs), Number::Float(rhs)) => Ok((lhs / rhs).map(Number::Float)),
            (Number::Float(lhs), Number::Integer(rhs)) => Ok((lhs / rhs).map(Number::Float)),
            _ => Err(InvalidArithmeticError),
        }
    }
}
impl core::ops::Rem for Term {
    type Output = Result<Result<Integer, DivisionError>, InvalidArithmeticError>;

    fn rem(self, rhs: Self) -> Self::Output {
        let lhs: Integer = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Integer = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        Ok(lhs % rhs)
    }
}
impl core::ops::Neg for Term {
    type Output = Result<Number, InvalidArithmeticError>;

    fn neg(self) -> Self::Output {
        let lhs: Number = self.try_into().map_err(|_| InvalidArithmeticError)?;
        Ok(-lhs)
    }
}

impl core::ops::Shl for Term {
    type Output = Result<Integer, InvalidArithmeticError>;

    fn shl(self, rhs: Self) -> Self::Output {
        let lhs: Integer = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Integer = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        Ok((lhs << rhs).unwrap())
    }
}
impl core::ops::Shr for Term {
    type Output = Result<Integer, InvalidArithmeticError>;

    fn shr(self, rhs: Self) -> Self::Output {
        let lhs: Integer = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Integer = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        Ok((lhs >> rhs).unwrap())
    }
}
impl core::ops::BitAnd for Term {
    type Output = Result<Integer, InvalidArithmeticError>;

    fn bitand(self, rhs: Self) -> Self::Output {
        let lhs: Integer = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Integer = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        Ok(lhs & rhs)
    }
}
impl core::ops::BitOr for Term {
    type Output = Result<Integer, InvalidArithmeticError>;

    fn bitor(self, rhs: Self) -> Self::Output {
        let lhs: Integer = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Integer = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        Ok(lhs | rhs)
    }
}
impl core::ops::BitXor for Term {
    type Output = Result<Integer, InvalidArithmeticError>;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let lhs: Integer = self.try_into().map_err(|_| InvalidArithmeticError)?;
        let rhs: Integer = rhs.try_into().map_err(|_| InvalidArithmeticError)?;

        Ok(lhs ^ rhs)
    }
}

/*
#[cfg(test)]
mod test {
    use core::alloc::Layout;
    use core::ptr::NonNull;

    use crate::process::ProcessHeap;

    use super::*;

    macro_rules! cons {
        ($heap:expr, $tail:expr) => {{
            cons!($heap, $tail, Term::Nil)
        }};

        ($heap:expr, $head:expr, $tail:expr) => {{
            let layout = Layout::new::<Cons>();
            let ptr: NonNull<Cons> = $heap.allocate(layout).unwrap().cast();
            ptr.as_ptr().write(Cons::cons($head, $tail));
            Term::Cons(ptr)
        }};

        ($heap:expr, $head:expr, $tail:expr, $($rest:expr,)+) => {{
            let rest = cons!($heap, $($rest),+);
            let tail = cons!($heap, $tail, tail);
            cons!($heap, $head, tail);
        }}
    }

    #[test]
    fn list_test() {
        let heap = ProcessHeap::new();

        let list = cons!(
            &heap,
            Term::Binary(Binary::from_str("foo")),
            Term::Float(f64::MIN.into()),
            Term::Int(42),
        );

        let opaque: OpaqueTerm = list.into();
        let value: Term = opaque.into();
        let cons: NonNull<Cons> = value.try_into().unwrap();
        let mut iter = cons.iter();

        assert_eq!(iter.next(), Some(Ok(Term::Binary(Binary::from_str("foo")))));
        assert_eq!(iter.next(), Some(Ok(Term::Float(f64::MIN.into()))));
        assert_eq!(iter.next(), Some(Ok(Term::Int(42))));
        assert_eq!(iter.next(), Some(Ok(Term::Nil)));
        assert_eq!(iter.next(), None);
    }
}
*/
