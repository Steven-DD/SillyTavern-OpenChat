package io.github.stevendd.sillytavernopenchat

import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    ViewCompat.setOnApplyWindowInsetsListener(window.decorView) { _, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
      val gestures = insets.getInsets(WindowInsetsCompat.Type.systemGestures())
      val density = resources.displayMetrics.density
      if (density > 0f) {
        val rawTopDp = (bars.top / density).toInt()
        val topDp = if (rawTopDp > 0) rawTopDp else 38

        val rawBottomDp = maxOf((bars.bottom / density).toInt(), (gestures.bottom / density).toInt())
        val bottomDp = if (rawBottomDp > 0) rawBottomDp else 24

        findWebView(window.decorView)?.let { wv ->
          wv.post {
            wv.evaluateJavascript(
              "document.documentElement.style.setProperty('--sat', '${topDp}px'); document.documentElement.style.setProperty('--sab', '${bottomDp}px');",
              null
            )
          }
        }
      }
      insets
    }
  }

  private fun findWebView(view: View): WebView? {
    if (view is WebView) return view
    if (view is ViewGroup) {
      for (i in 0 until view.childCount) {
        val found = findWebView(view.getChildAt(i))
        if (found != null) return found
      }
    }
    return null
  }
}
