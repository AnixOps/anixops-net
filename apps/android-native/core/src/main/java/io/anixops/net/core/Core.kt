package io.anixops.net.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonObject

/**
 * Typed commands over the client. Every call runs on [dispatcher] (commands may block for long: downloads,
 * delay tests, the VPN consent) and throws [CoreException] when the client refuses.
 */
class Core(
    private val transport: CoreTransport,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    suspend fun snapshot(): RuntimeSnapshot = call("runtime_snapshot", RuntimeSnapshot.serializer())

    suspend fun connect(): OperationResult = call("connect", OperationResult.serializer())

    suspend fun disconnect(): OperationResult = call("disconnect", OperationResult.serializer())

    suspend fun <T> call(
        command: String,
        answer: KSerializer<T>,
        args: JsonObject = Envelope.empty,
    ): T = withContext(dispatcher) {
        val text = transport.invoke(command, args.toString())
        val element = Envelope.read(text)
        try {
            Envelope.json.decodeFromJsonElement(answer, element)
        } catch (error: IllegalArgumentException) {
            throw CoreException("Unexpected answer to $command: ${error.message}")
        }
    }
}
