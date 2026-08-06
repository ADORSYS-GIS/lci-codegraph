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
}
