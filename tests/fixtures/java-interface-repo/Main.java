class Main {
    // Qualified call, matching this fixture set's house style (see java-repo's `Widget.build()`).
    String run() {
        return EnglishGreeter.greet();
    }

    // Bare call to the SAME single-implementation interface method. Before the fix, `greet` had two
    // candidates (the `Greeter` declaration and the `EnglishGreeter` implementation) and no qualifier
    // to disambiguate them, so this was dropped as ambiguous — issue #5. A declaration is now excluded
    // from the candidate set, leaving exactly one, so this resolves.
    String runBare() {
        return greet();
    }
}
