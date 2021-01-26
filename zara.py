import pyttsx3
import speech_recognition as sr
import datetime
import wikipedia
import webbrowser
import os
import smtplib
import query

engine = pyttsx3.init('sapi5')
voices = engine.getProperty('voices')
print(voices[1].id)
engine.setProperty('voice', voices[1].id)


def speak(audio):
    engine.say(audio)
    engine.runAndWait()


def wishMe():
    hour = int(datetime.datetime.now().hour)
    if hour >= 0 and hour < 12:
        speak("Good Morning!")
        print("Good Morning.")

    elif hour >= 12 and hour < 18:
        speak("Good Afternoon!")
        print("Good Afternoon.")
    else:
        speak("Good Evening!")
        print("Good Evening.")

    speak("hi this is Zara!.Please tell me how may I help you.")
    print("hi this is Zara!.Please tell me how may I help you.")


def takeCommand():

    r = sr.Recognizer()
    with sr.Microphone() as source:
        print("Listening...")
        r.pause_threshold = 1
        audio = r.listen(source)

    try:
        print("Recognizing...")
        query = r.recognize_google(audio, language='en-us')
        print(f"User said: {query}\n")

    except Exception as e:
        print(e)
        print("Say that again please...")
        return "none"
    return query



if __name__ == "__main__":
    wishMe()
    takeCommand()
    while True:
        query = takeCommand().lower()
        if 'wikipedia' in query:
            page = wikipedia.page("google")
            print(page.content)
            speak('searching wikipedia....')
            query = query.replace("wikipedia", "")
            results = wikipedia.summary(query, sentences=2)
            speak("Accoording to wikipedia")
            print(results)
            speak(results)

        elif 'open facebook' in query:
            webbrowser.open('www.facebook.com')
            speak('Opening Facebook')
            print('Opening Facebook')

        elif 'open youtube' in query:
            webbrowser.open('www.youtube.com')
            speak('Opening youtube...')
            print('Opening youtube...')

        elif 'play music' in query:
            speak('playing music...')
            print("playing music...")
            music_dir = 'D:\\songs\\favroit songs'
            songs = os.listdir(music_dir)
            print(songs)
            os.startfile(os.path.join(music_dir, songs[1]))

        elif 'open code' in query:
            speak("opening visual studio code ....")
            print("Opening visual Studio Code...")
            codePath = "C:\\Users\\ELCOT\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe"
            os.startfile(codePath)

       elif 'your name' in query:
           speak("My name is Zara! I am your personal assistant.")
           print("my name is Zara! I am your personal assistant.")

        elif 'how are you' in query:
            speak("Your voice perks me up, literally. How may i help you")
            print("Your voice perks me up, literally. How may i help you")

        elif 'today is my birthday' in query:
            speak("Happy birthday, God bless you!")
            print("Happy birthday, God bless you!")
        elif 'Tell me a joke ' in query:
            speak("there was a girl called zara,She use to tell a joke ")
            print("There was a girl called zara,She use to tell a joke ")

        elif 'the time' in query:
            speak("okay sir")
            strTime = datetime.datetime.now().strftime("%H:%M:%S")
            speak(f"Sir, the time is {strTime}")
            print(f"Sir, the time is {strTime}")

        elif 'open goolge' in query:
            speak("Opening Google...")
            print("Opening Google...")
            webbrowser.open("www.google.com")

        elif 'open portal' in query:
            webbrowser.open('portal.higrade.co.in')
            speak("Opening higrade...")
            print("Opening higrade...")

        elif 'open chrome' in query:
            speak("Opening chrome browser...")
            print("Opening chrome browser...")
            ch = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
            os.startfile(ch)

        elif 'open  overflow' in query:
            speak("Opening stack overflow!")
            print("Opening stackoverflow...")
            webbrowser.open('www.stackoverflow.com')

        elif 'open vlc' in query:
            speak("Opening Vlc media player...")
            print("Opening Vlc media player...")
            vlc = "C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe"
            os.startfile(vlc)

        elif 'open client' in query:
            speak('Opening SSH client...')
            print('Opening SSH client...')
            cl = "C:\\Program Files (x86)\\SSH Communications Security\\SSH Secure Shell\\SshClient.exe"
            os.startfile(cl)

        elif 'open moodle' in query:
            speak("Opening moddle...")
            print("Opening moodle...")
            webbrowser.open('http://lms.ceetle.co.in/login/index.php')

        elif 'show my hackerrank ' in query:
            speak("opening hackerrank")
            print("Opening HackerRank")
            webbrowser.open('https://hackerrank.com')

        elif 'stop' in query:
            speak("Okay sir, Have a nice day.")
            print("Okay sir, Have a nice day.")
            exit()
