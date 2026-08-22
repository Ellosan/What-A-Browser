package com.whatabrowser.wat.webview

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager

/**
 * Which way the phone is leaning, so the light on the glass can move with it.
 *
 * Apple's glass has a highlight that slides as the device turns, and that is
 * most of why it reads as a physical surface rather than a picture of one.
 * Nothing here is more complicated than an accelerometer: gravity's direction in
 * the phone's own frame *is* the tilt.
 *
 * No permission is involved — the motion sensors that need one are the step
 * counter and the heart rate monitor, not this. It runs only while a window is in
 * front, and stops in `onPause`, because a sensor left registered is a battery
 * complaint nobody can trace back to a highlight.
 */
class Tilt(context: Context, private val onChange: (x: Float, y: Float) -> Unit) :
    SensorEventListener {

    private val sensors = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager

    private val accelerometer: Sensor? = sensors?.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)

    private var x = 0f
    private var y = 0f

    val available: Boolean get() = accelerometer != null

    fun start() {
        val sensor = accelerometer ?: return
        // SENSOR_DELAY_UI, not GAME: this moves a highlight, and a faster stream
        // would cost more than the difference anyone could see.
        sensors?.registerListener(this, sensor, SensorManager.SENSOR_DELAY_UI)
    }

    fun stop() {
        sensors?.unregisterListener(this)
    }

    override fun onSensorChanged(event: SensorEvent) {
        if (event.values.size < 2) return
        val nextX = smooth(x, normalize(event.values[0]))
        val nextY = smooth(y, normalize(event.values[1]))
        // A highlight that jitters is worse than one that does not move at all,
        // so a change too small to see is not reported.
        if (kotlin.math.abs(nextX - x) < THRESHOLD && kotlin.math.abs(nextY - y) < THRESHOLD) return
        x = nextX
        y = nextY
        onChange(x, y)
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit

    companion object {
        /** Below this, a change is not worth a repaint. */
        const val THRESHOLD = 0.02f

        private const val SMOOTHING = 0.12f

        /** Gravity along one axis, as -1..1. */
        fun normalize(acceleration: Float): Float =
            (acceleration / SensorManager.GRAVITY_EARTH).coerceIn(-1f, 1f)

        /**
         * A low-pass filter. An accelerometer reading is mostly hand tremor, and
         * following it exactly makes the highlight shake.
         */
        fun smooth(previous: Float, sample: Float): Float =
            previous + (sample - previous) * SMOOTHING
    }
}
