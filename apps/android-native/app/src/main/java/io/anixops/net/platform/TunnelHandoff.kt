package io.anixops.net.platform

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * One VPN start: the caller waits for the interface's descriptor while the service builds it. Exactly one side owns
 * the descriptor at any time. The service holds it until [deliver] answers true; from then on it is the caller's.
 * When the caller stopped waiting first (timed out, or a newer start replaced this one), [deliver] answers false and
 * the service still owns the descriptor and must close it. A start therefore ends once, and no descriptor is left
 * open with nobody to close it (third-party review F06).
 */
class TunnelHandoff<T>(val request: T) {
    private val done = CountDownLatch(1)
    private var descriptor: Int? = null
    private var error: String? = null
    private var abandoned = false

    /** Hands [fd] to the waiting caller; false when the caller has left, and then the descriptor is still the service's. */
    @Synchronized
    fun deliver(fd: Int): Boolean {
        if (abandoned || done.count == 0L) return false
        descriptor = fd
        done.countDown()
        return true
    }

    /** Ends the start with [message]; nothing when it has ended already. */
    @Synchronized
    fun fail(message: String) {
        if (done.count == 0L) return
        error = message
        done.countDown()
    }

    /**
     * Waits up to [seconds] and answers the descriptor, which the caller then owns. Without one by then the start is
     * abandoned (a descriptor that comes later stays the service's) and this throws.
     */
    fun await(seconds: Long): Int {
        done.await(seconds, TimeUnit.SECONDS)
        synchronized(this) {
            descriptor?.let { return it }
            abandoned = true
            if (done.count > 0L) done.countDown()
            throw IllegalStateException(error ?: "The VPN did not start in time")
        }
    }
}
