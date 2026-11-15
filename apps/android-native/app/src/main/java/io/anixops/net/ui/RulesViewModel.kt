package io.anixops.net.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.DnsText
import io.anixops.net.core.MyRules
import io.anixops.net.core.NativeGroup
import io.anixops.net.core.OperationResult
import io.anixops.net.core.RuleSetStatus
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * What the rules page reads beyond the snapshot: the user's own rules, the profile's policy groups, the running
 * engine's rule sets and the custom DNS text with the Fake-IP switch. Null until read.
 */
data class RulesState(
    val myRules: MyRules? = null,
    val groups: List<NativeGroup> = emptyList(),
    val ruleSets: List<RuleSetStatus> = emptyList(),
    val dns: DnsText? = null,
    val busy: Boolean = false,
)

/** A sentence for the snackbar, or how many remote rule sets started downloading (worded by the page). */
sealed interface RulesEvent {
    data class Message(val text: String) : RulesEvent

    data class RuleSetsStarted(val count: Int) : RulesEvent
}

class RulesViewModel(private val core: Core) : ViewModel() {
    private val _state = MutableStateFlow(RulesState())
    val state: StateFlow<RulesState> = _state.asStateFlow()

    private val _events = Channel<RulesEvent>(Channel.BUFFERED)

    /** What commands answered, for a snackbar. */
    val events: Flow<RulesEvent> = _events.receiveAsFlow()

    /**
     * Reads the page, then the groups and rule sets every [POLL_MILLIS] until cancelled (a connection changes
     * both). The two texts are read once and after a save, so a poll never replaces what the user is typing.
     */
    suspend fun poll() {
        readTexts()
        while (true) {
            refresh()
            delay(POLL_MILLIS)
        }
    }

    fun saveMyRules(text: String) = perform { message(core.saveMyRules(text)) }

    fun selectGroupOutbound(group: String, outbound: String) =
        perform { message(core.selectGroupOutbound(group, outbound)) }

    /** Starts downloading the remote rule sets again. */
    fun updateRuleSets() = perform { RulesEvent.RuleSetsStarted(core.updateRuleSets()) }

    fun saveDns(text: DnsText) = perform { message(core.saveCustomDns(text)) }

    private fun message(result: OperationResult) = RulesEvent.Message(result.message)

    private suspend fun readTexts() {
        val rules = guard { core.myRules() }
        val dns = guard { core.customDns() }
        _state.update { it.copy(myRules = rules ?: it.myRules, dns = dns ?: it.dns) }
    }

    /** Quiet: a poll that fails keeps what the page shows rather than repeating the refusal every few seconds. */
    private suspend fun refresh() {
        try {
            val groups = core.nativeGroups()
            val ruleSets = core.ruleSets()
            _state.update { it.copy(groups = groups, ruleSets = ruleSets) }
        } catch (error: CoreException) {
            return
        }
    }

    private fun perform(command: suspend () -> RulesEvent) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                _events.send(command())
            } catch (error: CoreException) {
                _events.send(RulesEvent.Message(error.message.orEmpty()))
            } finally {
                readTexts()
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    private suspend fun <T> guard(read: suspend () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        _events.send(RulesEvent.Message(error.message.orEmpty()))
        null
    }

    companion object {
        private const val POLL_MILLIS = 3_000L
    }
}
