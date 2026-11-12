package app.pulse.android

import android.app.Application
import okhttp3.OkHttpClient
import org.schabi.newpipe.extractor.NewPipe
import java.util.concurrent.TimeUnit

class PulseApp : Application() {
    override fun onCreate() {
        super.onCreate()
        NewPipe.init(PulseDownloader(Net.client))
        Library.load(this)
    }
}

/** One connection pool for everything: Deezer, YouTube lookups and the audio itself. */
object Net {
    val client: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()
}
