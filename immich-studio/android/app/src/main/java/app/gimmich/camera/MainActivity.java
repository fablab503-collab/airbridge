package app.gimmich.camera;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.webkit.CookieManager;
import android.webkit.PermissionRequest;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * Gimmich Camera: opens the Studio camera of your own Gimmich (Immich) server full screen.
 * Photos and videos are sent straight to that server; the app never writes them to the phone.
 */
public class MainActivity extends Activity {
    private static final int ASK_CAMERA = 1;
    private static final String CAMERA_PATH = "/studio/camera/";

    private SharedPreferences prefs;
    private WebView web;
    private PermissionRequest waiting;
    private boolean cameFromLogin;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        prefs = getSharedPreferences("gimmich", MODE_PRIVATE);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        String server = prefs.getString("server", "");
        if (server.isEmpty()) showSetup(null);
        else openCamera(server);
    }

    /** First start (or the server could not be reached): ask for the Gimmich address. */
    private void showSetup(String problem) {
        LinearLayout box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setGravity(Gravity.CENTER);
        box.setPadding(64, 64, 64, 64);
        box.setBackgroundColor(Color.WHITE);

        TextView title = new TextView(this);
        title.setText("Gimmich Camera");
        title.setTextSize(28);
        title.setTextColor(0xFF4250AF);
        title.setGravity(Gravity.CENTER);

        TextView help = new TextView(this);
        help.setText((problem != null ? problem + "\n\n" : "")
                + "Type the https:// address of your Gimmich (for example https://imac.your-tailnet.ts.net). "
                + "Photos go straight there and are never saved on this phone.");
        help.setTextSize(15);
        help.setTextColor(0xFF444444);
        help.setPadding(0, 32, 0, 32);

        EditText input = new EditText(this);
        input.setInputType(InputType.TYPE_TEXT_VARIATION_URI);
        input.setHint("https://...");
        input.setText(prefs.getString("server", ""));
        input.setSingleLine(true);

        Button go = new Button(this);
        go.setText("Open camera");
        go.setOnClickListener(v -> {
            String url = normalise(input.getText().toString());
            if (url == null) {
                input.setError("Needs to start with https:// (phones only allow the camera on secure addresses)");
                return;
            }
            prefs.edit().putString("server", url).apply();
            openCamera(url);
        });

        box.addView(title);
        box.addView(help);
        box.addView(input);
        box.addView(go);
        setContentView(box);
    }

    static String normalise(String raw) {
        String url = raw.trim();
        if (url.isEmpty()) return null;
        if (!url.contains("://")) url = "https://" + url;
        Uri u = Uri.parse(url);
        if (!"https".equals(u.getScheme()) || u.getHost() == null || u.getHost().isEmpty()) return null;
        return "https://" + u.getHost() + (u.getPort() > 0 ? ":" + u.getPort() : "");
    }

    private void openCamera(String server) {
        web = new WebView(this);
        web.setBackgroundColor(Color.BLACK);
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setMediaPlaybackRequiresUserGesture(false);
        s.setAllowFileAccess(false);
        CookieManager.getInstance().setAcceptCookie(true);

        final String host = Uri.parse(server).getHost();
        web.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                // stay inside your own server; anything else opens in the normal browser
                if (host.equals(request.getUrl().getHost())) return false;
                startActivity(new android.content.Intent(android.content.Intent.ACTION_VIEW, request.getUrl()));
                return true;
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                CookieManager.getInstance().flush(); // stay logged in
                String path = Uri.parse(url).getPath();
                if (path != null && path.startsWith("/auth/login")) cameFromLogin = true;
                else if (cameFromLogin && path != null && !path.startsWith(CAMERA_PATH)) {
                    cameFromLogin = false; // just logged in: back to the camera
                    view.loadUrl(server + CAMERA_PATH);
                }
            }

            @Override
            public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame()) {
                    showSetup("Could not reach " + server + " (" + error.getDescription() + "). "
                            + "Is the iMac on and Tailscale connected?");
                }
            }
        });

        web.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onPermissionRequest(PermissionRequest request) {
                // only your own server may use the camera and microphone
                if (!host.equals(request.getOrigin().getHost())) {
                    request.deny();
                    return;
                }
                if (hasCameraPermission()) {
                    request.grant(request.getResources());
                } else {
                    waiting = request;
                    requestPermissions(new String[]{Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO}, ASK_CAMERA);
                }
            }
        });

        web.setSystemUiVisibility(View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        setContentView(web);
        web.loadUrl(server + CAMERA_PATH);
    }

    private boolean hasCameraPermission() {
        return checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED;
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        if (code != ASK_CAMERA || waiting == null) return;
        if (hasCameraPermission()) {
            waiting.grant(waiting.getResources());
        } else {
            waiting.deny();
            new AlertDialog.Builder(this)
                    .setMessage("Gimmich Camera needs the camera. Allow it in Settings → Apps → Gimmich Camera → Permissions.")
                    .setPositiveButton("OK", null).show();
        }
        waiting = null;
    }

    @Override
    public void onBackPressed() {
        if (web != null && web.canGoBack()) web.goBack();
        else super.onBackPressed();
    }

    @Override
    public boolean onKeyLongPress(int keyCode, android.view.KeyEvent event) {
        // long-press Back: change the server address
        if (keyCode == android.view.KeyEvent.KEYCODE_BACK) {
            showSetup(null);
            return true;
        }
        return super.onKeyLongPress(keyCode, event);
    }
}
