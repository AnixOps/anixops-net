package io.anixops.net.platform

/**
 * A name for a new file in a folder that does not overwrite one already there: [name] itself when free, else
 * `name (1).ext`, `name (2).ext`… as Android's own Downloads names copies. [taken] tells whether a name is in use.
 */
fun freeName(name: String, taken: (String) -> Boolean): String {
    if (!taken(name)) return name
    val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
    val base = name.substring(0, dot)
    val extension = name.substring(dot)
    var copy = 1
    while (taken("$base ($copy)$extension")) copy++
    return "$base ($copy)$extension"
}
