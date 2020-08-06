-module(init).
-export([start/0]).
-import(erlang, [display/1]).
-import(lumen, [is_big_integer/1]).

start() ->
  Sum = augend() + addend(),
  display(Sum).

augend() ->
  BigInteger = (1 bsl 46),
  display(is_big_integer(BigInteger)),
  display(BigInteger > 0),
  BigInteger.

addend() ->
  -179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0.


