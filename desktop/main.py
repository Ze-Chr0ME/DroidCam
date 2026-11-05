import sys

if __name__ == "__main__":
    if "--self-test" in sys.argv:
        from camerabridge.diagnostics import self_test
        self_test(sys.argv[sys.argv.index("--self-test") + 1])
    else:
        from camerabridge.app import main
        main()
