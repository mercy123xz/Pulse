sudo apt-get install -y -qq whois >/dev/null 2>&1
for d in pulse.gg pulse.fm pulse.app pulse.music pulse.so pulse.sh pulse.lol pulse.wtf pulse.rocks pulse.audio pulse.vip pulse.fyi pulse.cool pulse.page pulse.dev pulse.win pulse.bot pulse.ink pulse.best pulse.tv pulse.to pulse.cc pulse.ly pulse.im pulse.zone pulse.club pulse.live pulse.gay pulse.fans pulse.band pulse.radio pulse.studio pulse.tools pulse.software pulse.download pulse.exe pulse.mp3 \
 pulse.co.uk pulse.us.com pulse.uk.com pulse.eu.org pulse.com.co pulse.net.gg pulse.co.gg pulse.org.gg pulse.co.fm \
 listentopulse.com listentopulse.gg listentopulse.app getpulse.gg getpulse.app pulseplayer.gg pulseplayer.app pulseplayer.com pulsemusic.gg pulsemusic.app playpulse.gg pulsegg.com pulse-player.com pulseapp.gg; do
  code=$(curl -s -L -m 15 -o /tmp/r.json -w "%{http_code}" "https://rdap.org/domain/$d")
  w=$(timeout 15 whois "$d" 2>/dev/null | grep -iEm1 "no match|not found|no data found|no entries|available|status: free|is free|NOT FOUND|No Object Found|domain not found" | tr -d '\r' | cut -c1-60)
  echo "$d | rdap $code | whois: ${w:-registered?}"
done
