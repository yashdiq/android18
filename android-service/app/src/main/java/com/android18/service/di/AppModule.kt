package com.android18.service.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import kotlinx.serialization.json.Json
import javax.inject.Named
import javax.inject.Singleton

/**
 * Manual bindings Hilt can't derive from `@Inject` constructors alone.
 * Everything else (data sources, repositories, use cases) is constructor-
 * injected; this module only owns context-free singletons like the wire JSON.
 */
@Module
@InstallIn(SingletonComponent::class)
object AppModule {

    /**
     * JSON configured exactly like `android18-core`'s serde contract.
     *
     * `encodeDefaults = true` is load-bearing: `DeviceDto`'s
     * `transport`/`status`/`port` (and `EntryDto`'s `size`/`mtime` when 0)
     * are declared with default values, and kotlinx would silently strip
     * them from every response otherwise — the desktop's `GET /info`
     * deserialization failed on exactly this ("missing field
     * `transport`"). The desktop also tolerates their absence
     * (`#[serde(default)]`), so old APKs keep working.
     */
    @Provides
    @Singleton
    @Named("wire")
    fun wireJson(): Json = Json {
        encodeDefaults = true
        ignoreUnknownKeys = true
        explicitNulls = false
    }
}
