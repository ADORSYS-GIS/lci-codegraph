import { EnglishGreeter } from './english-greeter';

// Qualified call, matching this fixture set's house style (see typescript-repo's `Widget.build()`).
function run(): string {
  return EnglishGreeter.greet();
}

// Bare call to the SAME single-implementation interface method. Before the fix, `greet` had two
// candidates (the `Greeter` declaration and the `EnglishGreeter` implementation) and no qualifier to
// disambiguate them, so this was dropped as ambiguous — issue #5. A declaration is now excluded from
// the candidate set, leaving exactly one, so this resolves.
function runBare(): string {
  return greet();
}
