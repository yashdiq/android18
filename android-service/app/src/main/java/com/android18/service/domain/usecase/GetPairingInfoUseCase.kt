package com.android18.service.domain.usecase

import com.android18.service.data.local.PrefsDataSource
import com.android18.service.domain.model.PairingInfo
import javax.inject.Inject

/** Assembles the pairing record the desktop needs to connect. */
class GetPairingInfoUseCase @Inject constructor(
    private val prefs: PrefsDataSource,
) {
    operator fun invoke(ip: String?): PairingInfo = PairingInfo(
        ip = ip,
        port = prefs.port,
        token = prefs.token,
        deviceId = prefs.deviceId,
    )
}
