package org.ristle.spike;

import android.app.Activity;
import android.os.Bundle;
import android.util.Log;
import android.widget.TextView;

import java.io.FileOutputStream;

/** Runs the Rust spike in the app process and logs one JSON line per result. */
public class SpikeActivity extends Activity {
    static {
        System.loadLibrary("plugin_host_spike");
    }

    private static native String run(String sections, String modes);

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        TextView view = new TextView(this);
        view.setText("Running spike C...");
        setContentView(view);
        String sections = getIntent().getStringExtra("sections");
        String modes = getIntent().getStringExtra("modes");
        new Thread(() -> {
            Log.i("RISTLE_SPIKE", "START");
            String out = run(sections == null ? "" : sections, modes == null ? "" : modes);
            for (String line : out.split("\n")) {
                Log.i("RISTLE_SPIKE", line);
            }
            try (FileOutputStream f = new FileOutputStream(getFilesDir() + "/results.jsonl")) {
                f.write(out.getBytes("UTF-8"));
            } catch (Exception e) {
                Log.e("RISTLE_SPIKE", "write failed", e);
            }
            Log.i("RISTLE_SPIKE", "DONE");
            runOnUiThread(() -> view.setText("Done"));
        }).start();
    }
}
