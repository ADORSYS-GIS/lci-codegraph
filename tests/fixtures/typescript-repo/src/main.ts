import { caller } from './util';
import { Widget } from './service';
import { Gizmo } from './gizmo';
import { Lefty } from './lefty';

function run(): void {
  caller();
  Widget.build();
}

function runBare(): void {
  // `build` is defined on both Widget and Gadget with no qualifier here — must be
  // dropped as ambiguous, never guessed.
  build();
}

// Instance call through a variable receiver (issue #8): `spin` has exactly one definition in the
// repo (Gizmo.spin()), so this must resolve even though the variable is named `g`, not `Gizmo` —
// a value receiver carries no type information, and the call falls through to bare-name
// resolution, exactly like a bare `spin()` call would.
function runViaVariable(): void {
  const g = new Gizmo();
  g.spin();
}

// Negative: two same-named methods on different classes (Lefty.orbit()/Righty.orbit()), called
// through a variable receiver — must still be dropped as ambiguous. A value receiver must not
// turn "drop ambiguous" into "guess".
function runAmbiguousViaVariable(): void {
  const l = new Lefty();
  l.orbit();
}
