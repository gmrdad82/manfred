on run
	set desk to my deskPath()
	try
		do shell script quoted form of desk & " --first-launch"
	end try
	do shell script quoted form of desk & " > /dev/null 2>&1 &"
end run

on open theFiles
	set desk to my deskPath()
	try
		do shell script quoted form of desk & " --first-launch"
	end try
	repeat with theFile in theFiles
		do shell script quoted form of desk & " " & quoted form of (POSIX path of theFile) & " > /dev/null 2>&1 &"
	end repeat
end open

on deskPath()
	return (POSIX path of (path to me)) & "Contents/MacOS/manfred-desk"
end deskPath
