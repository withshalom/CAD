package com.withshalom.cad;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.Insets;
import android.graphics.Typeface;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.text.TextUtils;
import android.text.method.ScrollingMovementMethod;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Test viewer for the CAD core: opens a .blk or .dwg file and shows a short
 * summary and model space.
 *
 * <p>The drawing work happens in the native library (crates/cad-ffi). This
 * class only moves file descriptors and shows what comes back. It uses the
 * Android platform APIs alone, so the app contains no third-party code.
 */
public final class MainActivity extends Activity {
    private static final int PICK_DRAWING = 1;
    /** The drawing background, as in the core's SVG output. */
    private static final int BACKGROUND = 0xFF1B1C1E;
    private static final int WRITE = ParcelFileDescriptor.MODE_WRITE_ONLY
            | ParcelFileDescriptor.MODE_CREATE
            | ParcelFileDescriptor.MODE_TRUNCATE;

    /** Opens run one at a time, off the UI thread. */
    private final ExecutorService worker = Executors.newSingleThreadExecutor();

    private TextView title;
    private TextView info;
    private WebView drawing;
    /** Why the native library cannot be used, or null when it can. */
    private String nativeProblem;
    /** Number of the latest open; results of older ones are dropped. UI thread only. */
    private int latest;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        buildUi();
        nativeProblem = checkNative();
        if (nativeProblem != null) {
            info.setText(nativeProblem);
        } else if (!openFrom(getIntent())) {
            info.setText("Open a .blk or .dwg drawing.");
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        openFrom(intent);
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PICK_DRAWING && result == RESULT_OK && data != null
                && data.getData() != null) {
            open(data.getData());
        }
    }

    @Override
    protected void onDestroy() {
        worker.shutdownNow();
        ViewGroup parent = (ViewGroup) drawing.getParent();
        if (parent != null) {
            parent.removeView(drawing);
        }
        drawing.destroy();
        super.onDestroy();
    }

    // ------------------------------------------------------------ opening

    /** Open the file an "Open with" intent carries, if it carries one. */
    private boolean openFrom(Intent intent) {
        if (intent != null && Intent.ACTION_VIEW.equals(intent.getAction())
                && intent.getData() != null) {
            open(intent.getData());
            return true;
        }
        return false;
    }

    private void pick() {
        Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        pick.addCategory(Intent.CATEGORY_OPENABLE);
        pick.setType("*/*");
        try {
            startActivityForResult(pick, PICK_DRAWING);
        } catch (ActivityNotFoundException e) {
            info.setText("This device has no file picker.");
        }
    }

    private void open(Uri uri) {
        if (nativeProblem != null) {
            info.setText(nativeProblem);
            return;
        }
        final int job = ++latest;
        title.setText("CAD Viewer");
        info.setText("Opening…");
        drawing.loadUrl("about:blank");
        final File dir = new File(getCacheDir(), "view");
        worker.execute(() -> {
            // Asking the provider for the name can be slow (cloud storage),
            // so it happens here rather than on the UI thread.
            final String name = displayName(uri);
            runOnUiThread(() -> {
                if (!isDestroyed() && job == latest) {
                    title.setText(name);
                }
            });
            final Outcome outcome = render(uri, name, dir, job);
            runOnUiThread(() -> show(job, outcome));
        });
    }

    /** What an open produced: the text to show, and the page to load if any. */
    private static final class Outcome {
        final String text;
        final File page;

        Outcome(String text, File page) {
            this.text = text;
            this.page = page;
        }
    }

    /** Runs on the worker thread. */
    private Outcome render(Uri uri, String name, File dir, int job) {
        // Only the latest open's files are kept.
        File[] old = dir.listFiles();
        if (old != null) {
            for (File f : old) {
                f.delete();
            }
        }
        if (!dir.isDirectory() && !dir.mkdirs()) {
            return new Outcome("Could not prepare the app's cache folder.", null);
        }
        File infoFile = new File(dir, "info-" + job + ".txt");
        File svgFile = new File(dir, "drawing-" + job + ".svg");
        int status;
        try (ParcelFileDescriptor in = getContentResolver().openFileDescriptor(uri, "r");
                ParcelFileDescriptor infoOut = ParcelFileDescriptor.open(infoFile, WRITE);
                ParcelFileDescriptor svgOut = ParcelFileDescriptor.open(svgFile, WRITE)) {
            if (in == null) {
                return new Outcome("Could not open " + name + ".", null);
            }
            // The library takes the three descriptors over and closes them.
            status = Native.view(in.detachFd(), infoOut.detachFd(), svgOut.detachFd());
        } catch (IOException | RuntimeException e) {
            return new Outcome("Could not open " + name + ": " + e.getMessage(), null);
        }
        String text = readText(infoFile);
        if (status != Native.OK) {
            return new Outcome(describe(status) + (text.isEmpty() ? "" : "\n\n" + text), null);
        }
        File page = new File(dir, "view-" + job + ".html");
        try {
            Files.write(page.toPath(), pageHtml(svgFile.getName()).getBytes(StandardCharsets.UTF_8));
        } catch (IOException e) {
            return new Outcome(text + "\n\nCould not prepare the drawing: " + e.getMessage(), null);
        }
        return new Outcome(text, page);
    }

    /** Runs on the UI thread. */
    private void show(int job, Outcome outcome) {
        if (isDestroyed() || job != latest) {
            return; // another file was opened meanwhile
        }
        info.setText(outcome.text);
        info.scrollTo(0, 0);
        if (outcome.page != null) {
            drawing.loadUrl(Uri.fromFile(outcome.page).toString());
        }
    }

    /**
     * A page that fits the drawing to the screen and allows pinch zoom up
     * to 10x. The SVG is shown as an image, so nothing in it can run.
     */
    private static String pageHtml(String svgName) {
        return "<!DOCTYPE html><html><head><meta charset=\"utf-8\">"
                + "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1,"
                + " minimum-scale=1, maximum-scale=10\">"
                + "<style>html,body{margin:0;height:100%;background:#1b1c1e}"
                + "img{display:block;width:100%;height:100%;object-fit:contain}</style>"
                + "</head><body><img src=\"" + svgName + "\" alt=\"\"></body></html>";
    }

    private static String readText(File f) {
        try {
            return new String(Files.readAllBytes(f.toPath()), StandardCharsets.UTF_8).trim();
        } catch (IOException e) {
            return "";
        }
    }

    private static String describe(int status) {
        switch (status) {
            case Native.BAD_ARGUMENT:
                return "Internal error: a file was not handed over properly.";
            case Native.READ_FAILED:
                return "The file could not be read.";
            case Native.NOT_A_DRAWING:
                return "This is not a drawing this version can open.";
            case Native.WRITE_FAILED:
                return "The result could not be written. Is the device's storage full?";
            case Native.INTERNAL_ERROR:
                return "Internal error in the drawing engine. Please report it.";
            default:
                return "Unexpected result " + status + " from the drawing engine.";
        }
    }

    /** The name the file's provider gives it, for the title bar. */
    private String displayName(Uri uri) {
        String name = null;
        try (Cursor c = getContentResolver().query(
                uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) {
                name = c.getString(0);
            }
        } catch (RuntimeException e) {
            // Some providers do not answer; fall back to the URI below.
        }
        if (name == null) {
            name = uri.getLastPathSegment();
        }
        return name != null ? name : "drawing";
    }

    /** Null when the native library is present and matches this app. */
    private static String checkNative() {
        try {
            int version = Native.abiVersion();
            if (version != Native.ABI_VERSION) {
                return "The native library (interface " + version
                        + ") does not match this app (interface " + Native.ABI_VERSION
                        + "). Rebuild both.";
            }
            return null;
        } catch (LinkageError e) {
            return "The native library is missing or cannot be loaded (" + e.getMessage()
                    + "). Build it with `cargo xtask android`, then rebuild the app.";
        }
    }

    // ----------------------------------------------------------------- UI

    private void buildUi() {
        int pad = dp(8);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(BACKGROUND);

        LinearLayout bar = new LinearLayout(this);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setPadding(pad, pad, pad, pad);
        Button openButton = new Button(this);
        openButton.setText("Open");
        openButton.setOnClickListener(v -> pick());
        bar.addView(openButton);
        title = new TextView(this);
        title.setText("CAD Viewer");
        title.setTextColor(0xFFFFFFFF);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        title.setSingleLine(true);
        title.setEllipsize(TextUtils.TruncateAt.MIDDLE);
        title.setPadding(pad * 2, 0, 0, 0);
        bar.addView(title, new LinearLayout.LayoutParams(
                0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));
        root.addView(bar, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        info = new TextView(this);
        info.setTextColor(0xFFCCCCCC);
        info.setTypeface(Typeface.MONOSPACE);
        info.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        info.setMaxLines(8);
        info.setVerticalScrollBarEnabled(true);
        info.setMovementMethod(new ScrollingMovementMethod());
        info.setPadding(pad * 2, 0, pad * 2, pad);
        root.addView(info, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        drawing = new WebView(this);
        drawing.setBackgroundColor(BACKGROUND);
        WebSettings s = drawing.getSettings();
        s.setJavaScriptEnabled(false);
        s.setAllowFileAccess(true); // the pages this app writes to its cache
        s.setAllowContentAccess(false);
        s.setSupportZoom(true);
        s.setBuiltInZoomControls(true);
        s.setDisplayZoomControls(false);
        // Honour the page's viewport tag, which sets the zoom range.
        s.setUseWideViewPort(true);
        s.setLoadWithOverviewMode(true);
        root.addView(drawing, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        // From Android 15, apps targeting it draw behind the system bars;
        // keep the controls and the drawing clear of them.
        if (Build.VERSION.SDK_INT >= 30) {
            root.setOnApplyWindowInsetsListener((v, insets) -> {
                Insets bars = insets.getInsets(
                        WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
                v.setPadding(bars.left, bars.top, bars.right, bars.bottom);
                return WindowInsets.CONSUMED;
            });
        }
        setContentView(root);
    }

    private int dp(float value) {
        return Math.round(TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP, value, getResources().getDisplayMetrics()));
    }
}
