package com.withshalom.cad;

/**
 * Entry points of libcad_ffi.so, built from crates/cad-ffi by
 * {@code cargo xtask android}.
 *
 * <p>Only integers cross this boundary. Every file descriptor passed in
 * becomes the library's: it reads or writes through it and closes it,
 * whatever the outcome. Detach descriptors from their
 * {@link android.os.ParcelFileDescriptor} before passing them, and do not
 * use them afterwards.
 */
final class Native {
    /** Must equal {@code cad_ffi::ABI_VERSION}. */
    static final int ABI_VERSION = 1;

    // Status codes, as in cad_ffi::status.
    static final int OK = 0;
    static final int BAD_ARGUMENT = 1;
    static final int READ_FAILED = 2;
    static final int NOT_A_DRAWING = 3;
    static final int WRITE_FAILED = 4;
    static final int INTERNAL_ERROR = 5;

    static {
        System.loadLibrary("cad_ffi");
    }

    private Native() {}

    /** The interface version the library implements. */
    static native int abiVersion();

    /**
     * Read a drawing (.blk or .dwg) from {@code drawingFd}; write a short
     * plain-text summary to {@code infoFd} and model space as SVG to
     * {@code svgFd}. Returns a status code. When it fails, the summary
     * output holds the reason.
     */
    static native int view(int drawingFd, int infoFd, int svgFd);
}
