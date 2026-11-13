sudo apt-get install -y -qq whois dnsutils >/dev/null 2>&1
for d in pulse.gg pulse.fm pulse.app pulse.so pulse.sh pulse.cool pulse.page pulse.im pulse.fans pulse.band pulse.radio pulse.studio pulse.tools pulse.software pulse.lol pulse.wtf pulse.rocks pulse.audio pulse.fyi pulse.zone pulse.live \
 pulse.co.uk pulse.us.com pulse.uk.com pulse.eu.org pulse.com.co pulse.us.org pulse.gb.net pulse.co.com pulse.de.com pulse.net.gg pulse.co.gg pulse.org.gg pulse.co.fm \
 listentopulse.com listentopulse.gg listentopulse.app getpulse.gg pulseplayer.gg pulseplayer.app pulsemusic.gg pulsemusic.app playpulse.gg pulseapp.gg pulse-player.com; do
  ns=$(dig +short NS "$d" | head -1)
  w=$(timeout 15 whois "$d" 2>/dev/null | tr -d '\r')
  reg=$(echo "$w" | grep -iEm1 "^ *(Registrar:|Creation Date:|created:|Registered on:|registrar:)" | sed 's/  */ /g' | cut -c1-50)
  nf=$(echo "$w" | grep -iEm1 "no match|not found|no data found|no entries found|status: *free|is available|no object found" | cut -c1-40)
  prem=$(echo "$w" | grep -iEm1 "reserved|premium|not available for registration" | cut -c1-50)
  echo "$d | NS:${ns:-none} | ${reg:+REG($reg)} ${nf:+FREE($nf)} ${prem:+NOTE($prem)}"
  sleep 1
done
