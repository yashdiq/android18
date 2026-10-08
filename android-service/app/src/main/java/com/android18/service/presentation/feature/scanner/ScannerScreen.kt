package com.android18.service.presentation.feature.scanner

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Size
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.FlashlightOff
import androidx.compose.material.icons.filled.FlashlightOn
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.android18.service.data.server.FileService
import com.android18.service.presentation.common.SecondaryButton
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * QR pairing scanner: point at the desktop's onboarding QR, the phone
 * posts its identity to the desktop's one-shot listener and (on success)
 * starts the file server so the desktop's dial-back succeeds.
 */
@Composable
fun ScannerScreen(
    onDone: () -> Unit,
    viewModel: ScannerViewModel = hiltViewModel(),
) {
    val context = LocalContext.current
    val state by viewModel.state.collectAsStateWithLifecycle()
    val hint by viewModel.hint.collectAsStateWithLifecycle()
    val serverRunning by viewModel.serverRunning.collectAsStateWithLifecycle()

    var hasCamera by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED,
        )
    }
    val permission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted -> hasCamera = granted }

    LaunchedEffect(Unit) {
        if (!hasCamera) permission.launch(Manifest.permission.CAMERA)
    }

    // Allow starts the file server (the desktop dials back right away);
    // Deny leaves it stopped. Success hands control back to the dashboard.
    (state as? ScanState.Confirm)?.let { confirm ->
        AlertDialog(
            onDismissRequest = viewModel::deny,
            title = { Text("Allow this computer?") },
            text = { Text("Connect to the desktop at ${confirm.endpoint} and share your files with it?") },
            confirmButton = {
                TextButton(onClick = {
                    if (!serverRunning) {
                        context.startForegroundService(Intent(context, FileService::class.java))
                    }
                    viewModel.allow()
                }) { Text("Allow") }
            },
            dismissButton = { TextButton(onClick = viewModel::deny) { Text("Deny") } },
        )
    }
    LaunchedEffect(state) {
        if (state is ScanState.Success) {
            Toast.makeText(context, "Paired with desktop", Toast.LENGTH_SHORT).show()
            onDone()
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF020617)),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 4.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onDone) {
                Icon(
                    Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "Back",
                    tint = Color.White,
                )
            }
            Text(
                "Scan desktop QR",
                style = MaterialTheme.typography.titleMedium,
                color = Color.White,
            )
        }

        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .padding(16.dp)
                .clip(RoundedCornerShape(20.dp))
                .background(Color.Black),
        ) {
            if (hasCamera) {
                QrCameraPreview(onQr = viewModel::onQrScanned, enabled = state is ScanState.Idle)
            } else {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(24.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        "Camera access needed",
                        style = MaterialTheme.typography.titleSmall,
                        color = Color.White,
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "The scanner reads the pairing QR the desktop app shows. " +
                            "Camera is used only while this screen is open.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = Color(0xFF94A3B8),
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.height(16.dp))
                    SecondaryButton(
                        text = "Grant camera access",
                        onClick = { permission.launch(Manifest.permission.CAMERA) },
                    )
                }
            }
        }

        // Status line under the viewfinder.
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            shape = RoundedCornerShape(12.dp),
            color = Color(0xFF0F172A),
        ) {
            when (val s = state) {
                is ScanState.Idle -> StatusText(
                    hint ?: "Point at the QR in the desktop's pairing window",
                    // A hint means the viewfinder caught something that
                    // wasn't a pairing code; it clears on the next decode.
                    color = if (hint == null) Color(0xFFCBD5E1) else Color(0xFFFDA4AF),
                )
                is ScanState.Confirm -> StatusText("Waiting for your answer…")
                is ScanState.Starting -> StatusText("Starting the file server…")
                is ScanState.Posting -> StatusText("Pairing with ${s.endpoint}…")
                ScanState.Success -> StatusText("Paired ✓")
                is ScanState.Failed -> Column(
                    modifier = Modifier.padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        s.message,
                        style = MaterialTheme.typography.bodyMedium,
                        color = Color(0xFFFDA4AF),
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.height(8.dp))
                    SecondaryButton(text = "Try again", onClick = viewModel::reset)
                }
            }
        }
    }
}

@Composable
private fun StatusText(text: String, color: Color = Color(0xFFCBD5E1)) {
    Text(
        text,
        modifier = Modifier
            .fillMaxWidth()
            .padding(16.dp),
        style = MaterialTheme.typography.bodyMedium,
        color = color,
        textAlign = TextAlign.Center,
    )
}

/** CameraX preview + QR analysis; analysis pauses while a pairing posts. */
@Composable
private fun QrCameraPreview(onQr: (String) -> Unit, enabled: Boolean) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val previewView = remember { PreviewView(context) }
    val analyzerExecutor = remember { Executors.newSingleThreadExecutor() }
    val delivering = remember { AtomicBoolean(true) }
    var torchOn by remember { mutableStateOf(false) }
    val camera = remember { mutableStateOf<androidx.camera.core.Camera?>(null) }

    LaunchedEffect(enabled) { delivering.set(enabled) }

    DisposableEffect(Unit) {
        val providerFuture = ProcessCameraProvider.getInstance(context)
        providerFuture.addListener(
            {
                val provider = providerFuture.get()
                val preview = Preview.Builder()
                    .build()
                    .also { it.setSurfaceProvider(previewView.surfaceProvider) }
                // 720p analysis instead of the 640×480 default: the desktop's
                // compact QR renders ~8px/module at 300px, and the default
                // stream put modules near ZXing's decode floor — the "slow
                // scan" symptom. Closest-higher-then-lower keeps devices
                // without an exact 720p mode working.
                val analysis = ImageAnalysis.Builder()
                    .setResolutionSelector(
                        ResolutionSelector.Builder()
                            .setResolutionStrategy(
                                ResolutionStrategy(
                                    Size(1280, 720),
                                    ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER,
                                ),
                            )
                            .build(),
                    )
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
                    .also {
                        it.setAnalyzer(
                            analyzerExecutor,
                            QrAnalyzer(delivering) { raw -> onQr(raw) },
                        )
                    }
                provider.unbindAll()
                camera.value = try {
                    provider.bindToLifecycle(
                        lifecycleOwner,
                        CameraSelector.DEFAULT_BACK_CAMERA,
                        preview,
                        analysis,
                    )
                } catch (_: Exception) {
                    null
                }
            },
            ContextCompat.getMainExecutor(context),
        )
        onDispose {
            camera.value?.cameraControl?.enableTorch(false)
            analyzerExecutor.shutdown()
        }
    }

    Box(Modifier.fillMaxSize()) {
        AndroidView(factory = { previewView }, modifier = Modifier.fillMaxSize())
        IconButton(
            onClick = {
                torchOn = !torchOn
                camera.value?.cameraControl?.enableTorch(torchOn)
            },
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(8.dp),
        ) {
            Icon(
                if (torchOn) Icons.Filled.FlashlightOn else Icons.Filled.FlashlightOff,
                contentDescription = if (torchOn) "Turn torch off" else "Turn torch on",
                tint = Color.White,
            )
        }
    }
}

/**
 * Decodes QR codes from the YUV luminance plane. `delivering` gates
 * callbacks (paused while a pairing request is in flight); frames are
 * throttled to ~10 decode attempts per second — plenty for a static
 * code, and short enough to catch a hand-off in passing.
 */
private class QrAnalyzer(
    private val delivering: AtomicBoolean,
    private val onQr: (String) -> Unit,
) : ImageAnalysis.Analyzer {

    private val reader = MultiFormatReader().apply {
        setHints(
            mapOf(
                DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
                DecodeHintType.TRY_HARDER to true,
            ),
        )
    }
    private var lastAttemptAt = 0L

    override fun analyze(image: ImageProxy) {
        image.use { frame ->
            val now = System.currentTimeMillis()
            if (now - lastAttemptAt < DEBOUNCE_MS) return
            lastAttemptAt = now
            if (!delivering.get()) return
            val plane = frame.planes.firstOrNull() ?: return
            val buffer = plane.buffer
            val bytes = ByteArray(buffer.remaining()).also { buffer.get(it) }
            // rowStride is the data width so padded rows decode correctly.
            val source = PlanarYUVLuminanceSource(
                bytes,
                plane.rowStride,
                frame.height,
                0,
                0,
                frame.width,
                frame.height,
                false,
            )
            try {
                val result = reader.decodeWithState(BinaryBitmap(HybridBinarizer(source)))
                if (delivering.getAndSet(false)) {
                    onQr(result.text)
                }
            } catch (_: Exception) {
                // No QR in this frame — the common case.
            } finally {
                reader.reset()
            }
        }
    }

    private companion object {
        const val DEBOUNCE_MS = 100L
    }
}