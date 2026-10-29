package dev.infermark.device

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import android.view.WindowManager
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class KeepScreenOnArgs {
    var on: Boolean = false
}

/**
 * Battery, thermal status, battery saver and chip name from Android's own APIs, for InferMark's
 * sensors (see `src/android.rs`). Apps can't read most of /sys and /proc on Android, so these
 * are the only reliable sources. Nothing here leaves the device.
 */
@TauriPlugin
class DevicePlugin(private val activity: Activity) : Plugin(activity) {

    @Command
    fun snapshot(invoke: Invoke) {
        val ret = JSObject()

        // The sticky battery broadcast: level, plug state, status, voltage, temperature.
        val battery = activity.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        if (battery != null) {
            val level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
            if (level >= 0 && scale > 0) ret.put("batteryPercent", level * 100.0 / scale)
            val plugged = battery.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) != 0
            ret.put("plugged", plugged)
            val status = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            ret.put(
                "discharging",
                !plugged && (status == BatteryManager.BATTERY_STATUS_DISCHARGING ||
                    status == BatteryManager.BATTERY_STATUS_NOT_CHARGING)
            )
            val mv = battery.getIntExtra(BatteryManager.EXTRA_VOLTAGE, -1)
            if (mv > 0) ret.put("voltageMv", mv)
            val tenths = battery.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, Int.MIN_VALUE)
            if (tenths != Int.MIN_VALUE) ret.put("batteryTempC", tenths / 10.0)
        }

        // Instantaneous current; Int.MIN_VALUE (or 0) when this phone doesn't report it.
        val bm = activity.getSystemService(Context.BATTERY_SERVICE) as? BatteryManager
        val current = bm?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CURRENT_NOW)
        if (current != null && current != Int.MIN_VALUE && current != 0) ret.put("current", current)

        val pm = activity.getSystemService(Context.POWER_SERVICE) as? PowerManager
        if (pm != null) {
            ret.put("powerSave", pm.isPowerSaveMode)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ret.put("thermalStatus", pm.currentThermalStatus)
            }
        }

        ret.put("manufacturer", Build.MANUFACTURER ?: "")
        ret.put("model", Build.MODEL ?: "")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            ret.put("socManufacturer", Build.SOC_MANUFACTURER)
            ret.put("socModel", Build.SOC_MODEL)
        }
        invoke.resolve(ret)
    }

    /** Keep the screen on during a run: Android dims, then throttles or pauses, otherwise. */
    @Command
    fun keepScreenOn(invoke: Invoke) {
        val args = invoke.parseArgs(KeepScreenOnArgs::class.java)
        activity.runOnUiThread {
            val flag = WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            if (args.on) activity.window.addFlags(flag) else activity.window.clearFlags(flag)
        }
        invoke.resolve()
    }
}
