package com.android18.service.data.repository

import android.content.Context
import android.os.Build
import android.provider.Settings
import com.android18.service.data.local.PrefsDataSource
import com.android18.service.data.server.dto.PairingQrDto
import com.android18.service.domain.usecase.GetPairingInfoUseCase
import com.android18.service.util.lanIpv4
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/** Pairing identity: endpoint, token — shared by UI, `/info`, and the
 *  scanner's desktop dial-back. */
@Singleton
class PairingRepository @Inject constructor(
    @ApplicationContext private val context: Context,
    private val prefs: PrefsDataSource,
    private val pairingInfo: GetPairingInfoUseCase,
) {

    fun info() = pairingInfo(lanIpv4())

    fun deviceName(): String =
        Settings.Global.getString(context.contentResolver, "device_name") ?: Build.MODEL

    fun regenerateToken(): String = prefs.regenerateToken()

    /** The identity payload posted to the desktop's pairing listener. */
    fun qrDto(): PairingQrDto {
        val info = info()
        return PairingQrDto(
            name = deviceName(),
            ip = info.ip ?: "",
            port = info.port,
            token = info.token,
            deviceId = info.deviceId,
        )
    }
}
