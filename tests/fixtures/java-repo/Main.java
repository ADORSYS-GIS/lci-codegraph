class Main {
    void run() {
        Util.caller();
        Widget.build();
    }

    void runBare() {
        // `build` is defined on both Widget and Gadget with no qualifier here — must be
        // dropped as ambiguous, never guessed.
        build();
    }

    // Instance call through a variable receiver (issue #8): `spin` has exactly one definition
    // in the repo (Gizmo.spin()), so this must resolve even though the variable is named `g`,
    // not `Gizmo` — a value receiver carries no type information, and the call falls through to
    // bare-name resolution, exactly like a bare `spin()` call would.
    void runViaVariable() {
        Gizmo g = new Gizmo();
        g.spin();
    }

    // Negative: two same-named methods on different classes (Lefty.orbit()/Righty.orbit()),
    // called through a variable receiver — must still be dropped as ambiguous. A value receiver
    // must not turn "drop ambiguous" into "guess".
    void runAmbiguousViaVariable() {
        Lefty l = new Lefty();
        l.orbit();
    }
}
