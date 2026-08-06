const { caller } = require('./util');
const { Widget } = require('./widgets');

function run() {
  caller();
  Widget.build();
}

function runBare() {
  // `build` is defined on both Widget and Gadget with no qualifier here — must be
  // dropped as ambiguous, never guessed.
  build();
}
