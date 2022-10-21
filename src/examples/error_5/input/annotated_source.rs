fn main() {
  let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("Hello");
  let <tspan data-hash="2">y</tspan> = <tspan data-hash="1">&amp;mut x</tspan>; 
  let <tspan data-hash="3">z</tspan> = <tspan data-hash="1">&amp;mut x</tspan>; // ERROR: y is still live
  <tspan class="fn" data-hash="0" hash="6">String::push_str</tspan>(<tspan data-hash="2">y</tspan>, ", world");
  <tspan class="fn" data-hash="0" hash="6">String::push_str</tspan>(<tspan data-hash="3">z</tspan>, ", friend");
  <tspan class="fn" data-hash="0" hash="5">println!</tspan>("{}", <tspan data-hash="1">x</tspan>);
}